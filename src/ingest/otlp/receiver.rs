use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use opentelemetry_proto::tonic::collector::{
    logs::v1::{ExportLogsServiceRequest, ExportLogsServiceResponse},
    trace::v1::{ExportTraceServiceRequest, ExportTraceServiceResponse},
};
use prost::Message;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    adapters::agents::{AdapterRegistry, CursorAgentAdapter},
    core::{model::AgentEvent, store::TelemetryStore},
};

use super::normalize::{normalize_logs, normalize_traces};

const CANONICAL_EVENT_BATCH_SIZE: usize = 500;
const CANONICAL_EVENT_BODY_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CanonicalEventsPayload {
    One(AgentEvent),
    Many(Vec<AgentEvent>),
}

#[derive(Clone)]
struct ReceiverState {
    store: Arc<dyn TelemetryStore>,
    adapters: AdapterRegistry,
}

pub async fn serve(bind: SocketAddr, store: Arc<dyn TelemetryStore>) -> Result<()> {
    let state = ReceiverState {
        store,
        adapters: AdapterRegistry::default(),
    };
    let app = Router::new()
        .route("/v1/logs", post(receive_logs))
        .route("/v1/traces", post(receive_traces))
        .route(
            "/v1/events",
            post(receive_events).layer(DefaultBodyLimit::max(CANONICAL_EVENT_BODY_LIMIT)),
        )
        .route("/v1/hooks/cursor", post(receive_cursor_hook))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind OTLP receiver on {bind}"))?;

    println!("OTLP HTTP/protobuf receiver listening on http://{bind}");
    axum::serve(listener, app)
        .await
        .context("OTLP receiver failed")
}

async fn receive_events(
    State(state): State<ReceiverState>,
    Json(payload): Json<CanonicalEventsPayload>,
) -> Response {
    let mut events = match canonical_events(payload) {
        Ok(events) => events,
        Err(message) => return bad_request(message),
    };
    for event in &mut events {
        event.hydrate_token_usage();
    }

    if let Err(error) = state.store.append(&events).await {
        return server_error(format!("failed to store canonical events: {error:#}"));
    }

    StatusCode::NO_CONTENT.into_response()
}

fn canonical_events(payload: CanonicalEventsPayload) -> Result<Vec<AgentEvent>, String> {
    let events = match payload {
        CanonicalEventsPayload::One(event) => vec![event],
        CanonicalEventsPayload::Many(events) => events,
    };
    if events.is_empty() {
        return Err("canonical event batch must not be empty".into());
    }
    if events.len() > CANONICAL_EVENT_BATCH_SIZE {
        return Err(format!(
            "canonical event batch exceeds maximum of {CANONICAL_EVENT_BATCH_SIZE}"
        ));
    }
    Ok(events)
}

async fn receive_cursor_hook(
    State(state): State<ReceiverState>,
    Json(payload): Json<Value>,
) -> Response {
    let adapter = CursorAgentAdapter;
    let Some(mut event) = adapter.normalize_hook(&payload) else {
        return bad_request("invalid Cursor hook payload".into());
    };
    event.hydrate_token_usage();

    if let Err(error) = state.store.append(&[event]).await {
        return server_error(format!("failed to store Cursor hook event: {error:#}"));
    }

    StatusCode::NO_CONTENT.into_response()
}

async fn receive_logs(State(state): State<ReceiverState>, body: Bytes) -> Response {
    let request = match ExportLogsServiceRequest::decode(body) {
        Ok(request) => request,
        Err(error) => return bad_request(format!("invalid OTLP logs protobuf: {error}")),
    };

    let events = normalize_logs(&state.adapters, request);
    if let Err(error) = state.store.append(&events).await {
        return server_error(format!("failed to store OTLP logs: {error:#}"));
    }

    protobuf_response(ExportLogsServiceResponse::default())
}

async fn receive_traces(State(state): State<ReceiverState>, body: Bytes) -> Response {
    let request = match ExportTraceServiceRequest::decode(body) {
        Ok(request) => request,
        Err(error) => return bad_request(format!("invalid OTLP traces protobuf: {error}")),
    };

    let events = normalize_traces(&state.adapters, request);
    if let Err(error) = state.store.append(&events).await {
        return server_error(format!("failed to store OTLP traces: {error:#}"));
    }

    protobuf_response(ExportTraceServiceResponse::default())
}

fn protobuf_response(message: impl Message) -> Response {
    let mut encoded = Vec::new();
    if message.encode(&mut encoded).is_err() {
        return server_error("failed to encode OTLP response".into());
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-protobuf")
        .body(Body::from(encoded))
        .unwrap()
}

fn bad_request(message: String) -> Response {
    (StatusCode::BAD_REQUEST, message).into_response()
}

fn server_error(message: String) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use crate::core::model::{AgentEvent, AgentEventKind, DecisionContext};

    use super::{CanonicalEventsPayload, canonical_events};

    fn decision_event() -> AgentEvent {
        let mut event =
            AgentEvent::new("apocrypha", AgentEventKind::Decision, "agent_trace_triage");
        event.timestamp = Utc::now();
        event.trace_id = Some("trace-1".into());
        event.model = Some("jev-1.13.0".into());
        event.decision = Some(DecisionContext {
            question: Some("agent trace triage".into()),
            evidence: vec![],
            alternatives: vec![
                "HEALTHY".into(),
                "REVIEW".into(),
                "RETRY".into(),
                "INCIDENT".into(),
            ],
            selected: Some("REVIEW".into()),
            constraints: vec![],
            assumptions: vec![],
            confidence: Some(0.82),
            probabilities: [
                ("HEALTHY".into(), 0.10),
                ("REVIEW".into(), 0.82),
                ("RETRY".into(), 0.06),
                ("INCIDENT".into(), 0.02),
            ]
            .into_iter()
            .collect(),
            risk: Some(0.5),
            provider: Some("cloudflare-workers-ai".into()),
            route: vec!["rule".into(), "jev".into()],
            expected_outcome: None,
            details: json!({
                "answers": {
                    "triage": {
                        "type": "choice",
                        "value": "REVIEW"
                    }
                }
            }),
        });
        event
    }

    #[test]
    fn canonical_ingest_accepts_one_event() {
        let events = canonical_events(CanonicalEventsPayload::One(decision_event())).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AgentEventKind::Decision);
    }

    #[test]
    fn canonical_ingest_accepts_batches() {
        let events = canonical_events(CanonicalEventsPayload::Many(vec![
            decision_event(),
            decision_event(),
        ]))
        .unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn canonical_ingest_rejects_empty_batches() {
        let error = canonical_events(CanonicalEventsPayload::Many(vec![])).unwrap_err();
        assert!(error.contains("must not be empty"));
    }

    #[test]
    fn canonical_ingest_rejects_oversized_batches() {
        let error = canonical_events(CanonicalEventsPayload::Many(
            (0..501).map(|_| decision_event()).collect(),
        ))
        .unwrap_err();
        assert!(error.contains("maximum of 500"));
    }
}
