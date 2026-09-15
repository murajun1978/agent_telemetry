use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use opentelemetry_proto::tonic::collector::{
    logs::v1::{ExportLogsServiceRequest, ExportLogsServiceResponse},
    trace::v1::{ExportTraceServiceRequest, ExportTraceServiceResponse},
};
use prost::Message;
use serde_json::Value;

use crate::{
    adapters::agents::{AdapterRegistry, CursorAgentAdapter},
    core::store::TelemetryStore,
};

use super::normalize::{normalize_logs, normalize_traces};

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
