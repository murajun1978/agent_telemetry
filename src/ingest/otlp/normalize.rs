use opentelemetry_proto::tonic::collector::{
    logs::v1::ExportLogsServiceRequest, trace::v1::ExportTraceServiceRequest,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{adapters::agents::AdapterRegistry, core::model::AgentEvent};

use super::{
    convert::{any_value_to_json, attributes_to_json, id_or_none, unix_nanos},
    types::{OtlpLogRecord, OtlpSpanRecord},
};

pub(super) fn normalize_logs(
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
                    event.hydrate_token_usage();
                    events.push(event);
                }
            }
        }
    }
    events
}

pub(super) fn normalize_traces(
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
                let duration_ms =
                    span.end_time_unix_nano
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
                    event.hydrate_token_usage();
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
