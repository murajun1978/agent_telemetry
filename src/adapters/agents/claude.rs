use serde_json::{Value, json};

use crate::{
    adapters::agents::SemanticAdapter,
    core::model::{AgentEvent, AgentEventKind, DecisionContext},
    ingest::otlp::{OtlpLogRecord, OtlpSpanRecord},
};

pub struct ClaudeCodeAdapter;

impl SemanticAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn normalize_log(&self, record: &OtlpLogRecord) -> Option<AgentEvent> {
        if !record.event_name.starts_with("claude_code.") {
            return None;
        }

        let event = record
            .event_name
            .strip_prefix("claude_code.")
            .unwrap_or(&record.event_name);
        let kind = match event {
            "tool_result" => AgentEventKind::ToolCall,
            "tool_decision" => AgentEventKind::Decision,
            "api_request" | "assistant_response" => AgentEventKind::LlmCall,
            "api_error" | "api_refusal" | "internal_error" | "api_retries_exhausted" => {
                AgentEventKind::Error
            }
            "user_prompt" => AgentEventKind::Observation,
            _ => AgentEventKind::Log,
        };

        let mut canonical = AgentEvent::new(self.name(), kind, event);
        canonical.timestamp = record.timestamp;
        canonical.agent_version = string_attr(&record.attributes, "app.version")
            .or_else(|| string_attr(&record.resource_attributes, "service.version"));
        canonical.session_id = string_attr(&record.attributes, "session.id");
        canonical.turn_id = string_attr(&record.attributes, "prompt.id");
        canonical.trace_id = record.trace_id.clone();
        canonical.span_id = record.span_id.clone();
        canonical.model = string_attr(&record.attributes, "model")
            .or_else(|| string_attr(&record.attributes, "gen_ai.request.model"));
        canonical.tool_name = string_attr(&record.attributes, "tool_name")
            .or_else(|| string_attr(&record.attributes, "gen_ai.tool.name"));
        canonical.duration_ms = number_attr(&record.attributes, "duration_ms");
        canonical.input_tokens = u64_attr(&record.attributes, "input_tokens");
        canonical.output_tokens = u64_attr(&record.attributes, "output_tokens");
        canonical.cost_usd = cost_attr(&record.attributes);
        canonical.status = success_status(&record.attributes)
            .or_else(|| string_attr(&record.attributes, "status"));

        if event == "tool_decision" {
            canonical.decision = Some(DecisionContext {
                question: canonical
                    .tool_name
                    .as_ref()
                    .map(|name| format!("Allow tool `{name}` to execute?")),
                evidence: string_attr(&record.attributes, "source")
                    .or_else(|| string_attr(&record.attributes, "decision_source"))
                    .into_iter()
                    .collect(),
                alternatives: vec!["accept".into(), "reject".into()],
                selected: string_attr(&record.attributes, "decision")
                    .or_else(|| string_attr(&record.attributes, "decision_type")),
                constraints: Vec::new(),
                assumptions: Vec::new(),
                confidence: None,
                expected_outcome: None,
            });
        }

        canonical.attributes = Value::Object(record.attributes.clone());
        canonical.raw = json!({
            "signal": "log",
            "event_name": record.event_name,
            "resource": record.resource_attributes,
            "body": record.body,
        });
        Some(canonical)
    }

    fn normalize_span(&self, record: &OtlpSpanRecord) -> Option<AgentEvent> {
        if !record.name.starts_with("claude_code.") {
            return None;
        }

        let span_name = record
            .name
            .strip_prefix("claude_code.")
            .unwrap_or(&record.name);
        let kind = match span_name {
            "llm_request" => AgentEventKind::LlmCall,
            "tool" | "tool.execution" => AgentEventKind::ToolCall,
            "tool.blocked_on_user" => AgentEventKind::Decision,
            "interaction" => AgentEventKind::Trace,
            _ => AgentEventKind::Trace,
        };

        let mut canonical = AgentEvent::new(self.name(), kind, span_name);
        canonical.timestamp = record.timestamp;
        canonical.agent_version = string_attr(&record.attributes, "app.version")
            .or_else(|| string_attr(&record.resource_attributes, "service.version"));
        canonical.session_id = string_attr(&record.attributes, "session.id");
        canonical.turn_id = string_attr(&record.attributes, "prompt.id");
        canonical.trace_id = Some(record.trace_id.clone());
        canonical.span_id = Some(record.span_id.clone());
        canonical.model = string_attr(&record.attributes, "model")
            .or_else(|| string_attr(&record.attributes, "gen_ai.request.model"));
        canonical.tool_name = string_attr(&record.attributes, "tool_name")
            .or_else(|| string_attr(&record.attributes, "gen_ai.tool.name"));
        canonical.duration_ms =
            number_attr(&record.attributes, "duration_ms").or(Some(record.duration_ms));
        canonical.input_tokens = u64_attr(&record.attributes, "input_tokens");
        canonical.output_tokens = u64_attr(&record.attributes, "output_tokens");
        canonical.cost_usd = cost_attr(&record.attributes);
        canonical.status = record
            .status
            .clone()
            .or_else(|| success_status(&record.attributes));

        if span_name == "tool.blocked_on_user" {
            canonical.decision = Some(DecisionContext {
                question: Some("Allow tool execution?".into()),
                evidence: string_attr(&record.attributes, "source")
                    .or_else(|| string_attr(&record.attributes, "decision_source"))
                    .into_iter()
                    .collect(),
                alternatives: vec!["accept".into(), "reject".into()],
                selected: string_attr(&record.attributes, "decision")
                    .or_else(|| string_attr(&record.attributes, "decision_type")),
                constraints: Vec::new(),
                assumptions: Vec::new(),
                confidence: None,
                expected_outcome: None,
            });
        }

        canonical.attributes = Value::Object(record.attributes.clone());
        canonical.raw = json!({
            "signal": "trace",
            "span_name": record.name,
            "parent_span_id": record.parent_span_id,
            "resource": record.resource_attributes,
        });
        Some(canonical)
    }
}

fn string_attr(attributes: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    attributes.get(key).and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    })
}

fn number_attr(attributes: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    attributes.get(key).and_then(|value| match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    })
}

fn u64_attr(attributes: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    attributes.get(key).and_then(|value| match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    })
}

fn cost_attr(attributes: &serde_json::Map<String, Value>) -> Option<f64> {
    number_attr(attributes, "cost_usd").or_else(|| {
        u64_attr(attributes, "cost_usd_micros").map(|micros| micros as f64 / 1_000_000.0)
    })
}

fn success_status(attributes: &serde_json::Map<String, Value>) -> Option<String> {
    attributes.get("success").and_then(|value| match value {
        Value::Bool(true) => Some("success".into()),
        Value::Bool(false) => Some("error".into()),
        Value::String(value) if value == "true" => Some("success".into()),
        Value::String(value) if value == "false" => Some("error".into()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::{Map, Value, json};

    use super::{ClaudeCodeAdapter, SemanticAdapter};
    use crate::{core::model::AgentEventKind, ingest::otlp::OtlpLogRecord};

    #[test]
    fn normalizes_tool_result() {
        let mut attributes = Map::new();
        attributes.insert("session.id".into(), json!("session-1"));
        attributes.insert("tool_name".into(), json!("Bash"));
        attributes.insert("success".into(), json!("true"));
        attributes.insert("duration_ms".into(), json!(12.5));

        let event = ClaudeCodeAdapter
            .normalize_log(&OtlpLogRecord {
                event_name: "claude_code.tool_result".into(),
                timestamp: Utc::now(),
                trace_id: None,
                span_id: None,
                attributes,
                resource_attributes: Map::new(),
                body: Value::Null,
            })
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::ToolCall);
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.tool_name.as_deref(), Some("Bash"));
        assert_eq!(event.status.as_deref(), Some("success"));
    }

    #[test]
    fn normalizes_canonical_tool_decision_keys() {
        let mut attributes = Map::new();
        attributes.insert("decision".into(), json!("accept"));
        attributes.insert("source".into(), json!("user"));

        let event = ClaudeCodeAdapter
            .normalize_log(&OtlpLogRecord {
                event_name: "claude_code.tool_decision".into(),
                timestamp: Utc::now(),
                trace_id: None,
                span_id: None,
                attributes,
                resource_attributes: Map::new(),
                body: Value::Null,
            })
            .unwrap();

        let decision = event.decision.unwrap();
        assert_eq!(decision.selected.as_deref(), Some("accept"));
        assert_eq!(decision.evidence, vec!["user"]);
    }
}
