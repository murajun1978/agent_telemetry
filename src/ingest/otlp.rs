use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::{DateTime, Utc};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::{ExportLogsServiceRequest, ExportLogsServiceResponse},
        trace::v1::{ExportTraceServiceRequest, ExportTraceServiceResponse},
    },
    common::v1::{AnyValue, KeyValue, any_value},
};
use prost::Message;
use serde_json::{Map, Number, Value};
use uuid::Uuid;

use crate::{
    adapters::agents::AdapterRegistry,
    core::{model::AgentEvent, store::TelemetryStore},
};

#[derive(Debug, Clone)]
pub struct OtlpLogRecord {
    pub event_name: String,
    pub timestamp: DateTime<Utc>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub attributes: Map<String, Value>,
    pub resource_attributes: Map<String, Value>,
    pub body: Value,
}

#[derive(Debug, Clone)]
pub struct OtlpSpanRecord {
    pub name: String,
    pub timestamp: DateTime<Utc>,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub duration_ms: f64,
    pub status: Option<String>,
    pub attributes: Map<String, Value>,
    pub resource_attributes: Map<String, Value>,
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
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind OTLP receiver on {bind}"))?;

    println!("OTLP HTTP/protobuf receiver listening on http://{bind}");
    axum::serve(listener, app)
        .await
        .context("OTLP receiver failed")
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

fn normalize_logs(
    registry: &AdapterRegistry,
    request: ExportLogsServiceRequest,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    for resource_logs in request.resource_logs {
        let resource_attributes = resource_logs
            .resource
            .map(|resource| attributes_to_json(&resource.attributes))
            .unwrap_or_default();
        for scope_logs in resource_logs.scope_logs {
            for record in scope_logs.log_records {
                let attributes = attributes_to_json(&record.attributes);
                let event_name = if record.event_name.is_empty() {
                    attributes
                        .get("event.name")
                        .and_then(Value::as_str)
                        .unwrap_or("otel.log")
                        .to_owned()
                } else {
                    record.event_name
                };
                let source_time = if record.time_unix_nano != 0 {
                    record.time_unix_nano
                } else {
                    record.observed_time_unix_nano
                };
                let body = record
                    .body
                    .as_ref()
                    .map(any_value_to_json)
                    .unwrap_or(Value::Null);
                let normalized = OtlpLogRecord {
                    event_name,
                    timestamp: unix_nanos(source_time),
                    trace_id: id_or_none(&record.trace_id),
                    span_id: id_or_none(&record.span_id),
                    attributes,
                    resource_attributes: resource_attributes.clone(),
                    body,
                };
                if let Some(mut event) = registry.normalize_log(&normalized) {
                    event.id = stable_log_id(&normalized, source_time);
                    events.push(event);
                }
            }
        }
    }
    events
}

fn normalize_traces(
    registry: &AdapterRegistry,
    request: ExportTraceServiceRequest,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    for resource_spans in request.resource_spans {
        let resource_attributes = resource_spans
            .resource
            .map(|resource| attributes_to_json(&resource.attributes))
            .unwrap_or_default();
        for scope_spans in resource_spans.scope_spans {
            for span in scope_spans.spans {
                let duration_ms = span
                    .end_time_unix_nano
                    .saturating_sub(span.start_time_unix_nano) as f64
                    / 1_000_000.0;
                let status = span.status.as_ref().and_then(|status| match status.code {
                    1 => Some("success".to_owned()),
                    2 => Some("error".to_owned()),
                    _ => None,
                });
                let normalized = OtlpSpanRecord {
                    name: span.name,
                    timestamp: unix_nanos(span.start_time_unix_nano),
                    trace_id: hex::encode(span.trace_id),
                    span_id: hex::encode(span.span_id),
                    parent_span_id: id_or_none(&span.parent_span_id),
                    duration_ms,
                    status,
                    attributes: attributes_to_json(&span.attributes),
                    resource_attributes: resource_attributes.clone(),
                };
                if let Some(mut event) = registry.normalize_span(&normalized) {
                    event.id = stable_span_id(&normalized);
                    events.push(event);
                }
            }
        }
    }
    events
}

fn stable_log_id(record: &OtlpLogRecord, source_time: u64) -> String {
    let key = format!(
        "log|{}|{}|{}|{}|{}|{}|{}",
        record.event_name,
        source_time,
        record.trace_id.as_deref().unwrap_or(""),
        record.span_id.as_deref().unwrap_or(""),
        Value::Object(record.resource_attributes.clone()),
        Value::Object(record.attributes.clone()),
        record.body
    );
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()).to_string()
}

fn stable_span_id(record: &OtlpSpanRecord) -> String {
    let key = format!("trace|{}|{}", record.trace_id, record.span_id);
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()).to_string()
}

fn attributes_to_json(attributes: &[KeyValue]) -> Map<String, Value> {
    attributes
        .iter()
        .filter_map(|attribute| {
            attribute
                .value
                .as_ref()
                .map(|value| (attribute.key.clone(), any_value_to_json(value)))
        })
        .collect()
}

fn any_value_to_json(value: &AnyValue) -> Value {
    match value.value.as_ref() {
        Some(any_value::Value::StringValue(value)) => Value::String(value.clone()),
        Some(any_value::Value::StringValueStrindex(value)) => {
            Value::String(format!("#strindex:{value}"))
        }
        Some(any_value::Value::BoolValue(value)) => Value::Bool(*value),
        Some(any_value::Value::IntValue(value)) => Value::Number((*value).into()),
        Some(any_value::Value::DoubleValue(value)) => Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Some(any_value::Value::ArrayValue(value)) => {
            Value::Array(value.values.iter().map(any_value_to_json).collect())
        }
        Some(any_value::Value::KvlistValue(value)) => Value::Object(
            value
                .values
                .iter()
                .filter_map(|entry| {
                    entry
                        .value
                        .as_ref()
                        .map(|value| (entry.key.clone(), any_value_to_json(value)))
                })
                .collect(),
        ),
        Some(any_value::Value::BytesValue(value)) => Value::String(hex::encode(value)),
        None => Value::Null,
    }
}

fn unix_nanos(nanos: u64) -> DateTime<Utc> {
    if nanos == 0 {
        return Utc::now();
    }
    let seconds = (nanos / 1_000_000_000) as i64;
    let subsec_nanos = (nanos % 1_000_000_000) as u32;
    DateTime::<Utc>::from_timestamp(seconds, subsec_nanos).unwrap_or_else(Utc::now)
}

fn id_or_none(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| hex::encode(bytes))
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
