use serde_json::{Value, json};

use crate::{
    adapters::agents::SemanticAdapter,
    core::model::{AgentEvent, AgentEventKind},
    ingest::otlp::{OtlpLogRecord, OtlpSpanRecord},
};

pub struct GenericOtelAdapter;

impl SemanticAdapter for GenericOtelAdapter {
    fn name(&self) -> &'static str {
        "generic-otel"
    }

    fn normalize_log(&self, record: &OtlpLogRecord) -> Option<AgentEvent> {
        let agent = service_name(&record.resource_attributes).unwrap_or_else(|| self.name().into());
        let mut event = AgentEvent::new(agent, AgentEventKind::Log, &record.event_name);
        event.timestamp = record.timestamp;
        event.trace_id = record.trace_id.clone();
        event.span_id = record.span_id.clone();
        event.attributes = Value::Object(record.attributes.clone());
        event.raw = json!({
            "signal": "log",
            "resource": record.resource_attributes,
            "body": record.body,
        });
        Some(event)
    }

    fn normalize_span(&self, record: &OtlpSpanRecord) -> Option<AgentEvent> {
        let agent = service_name(&record.resource_attributes).unwrap_or_else(|| self.name().into());
        let mut event = AgentEvent::new(agent, AgentEventKind::Trace, &record.name);
        event.timestamp = record.timestamp;
        event.trace_id = Some(record.trace_id.clone());
        event.span_id = Some(record.span_id.clone());
        event.duration_ms = Some(record.duration_ms);
        event.status = record.status.clone();
        event.attributes = Value::Object(record.attributes.clone());
        event.raw = json!({
            "signal": "trace",
            "parent_span_id": record.parent_span_id,
            "resource": record.resource_attributes,
        });
        Some(event)
    }
}

fn service_name(attributes: &serde_json::Map<String, Value>) -> Option<String> {
    attributes
        .get("service.name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}
