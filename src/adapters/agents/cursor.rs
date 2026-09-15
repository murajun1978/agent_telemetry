use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    adapters::agents::SemanticAdapter,
    core::model::{AgentEvent, AgentEventKind, DecisionContext},
    ingest::otlp::{OtlpLogRecord, OtlpSpanRecord},
};

#[derive(Clone, Copy, Default)]
pub struct CursorAgentAdapter;

impl CursorAgentAdapter {
    pub fn normalize_hook(&self, payload: &Value) -> Option<AgentEvent> {
        let attributes = payload.as_object()?;
        let hook_name = string_attr(attributes, "hook_event_name")?;
        let kind = kind_for_hook(&hook_name);
        let is_decision = matches!(kind, AgentEventKind::Decision);

        let mut event = AgentEvent::new(self.name(), kind, hook_name.clone());
        event.id = stable_hook_id(attributes, &hook_name);
        event.agent_version = string_attr(attributes, "cursor_version");
        event.session_id = string_attr(attributes, "conversation_id");
        event.turn_id = string_attr(attributes, "generation_id");
        event.model = string_attr(attributes, "model_id").or_else(|| string_attr(attributes, "model"));
        event.tool_name = string_attr(attributes, "tool_name")
            .or_else(|| string_attr(attributes, "subagent_type"));
        event.duration_ms = number_attr(attributes, "duration")
            .or_else(|| number_attr(attributes, "duration_ms"));
        event.status = status_for_hook(attributes, &hook_name);

        if is_decision {
            event.decision = Some(DecisionContext {
                question: event
                    .tool_name
                    .as_ref()
                    .map(|tool| format!("Use Cursor tool `{tool}`?")),
                evidence: Vec::new(),
                alternatives: Vec::new(),
                selected: event.tool_name.clone(),
                constraints: Vec::new(),
                assumptions: Vec::new(),
                confidence: None,
                expected_outcome: None,
            });
        }

        let safe_attributes = safe_hook_attributes(attributes);
        event.attributes = Value::Object(safe_attributes.clone());
        event.raw = json!({
            "signal": "cursor_hook",
            "hook_event_name": hook_name,
            "metadata": safe_attributes,
            "content_redacted": true,
        });
        Some(event)
    }
}

impl SemanticAdapter for CursorAgentAdapter {
    fn name(&self) -> &'static str {
        "cursor-agent"
    }

    fn normalize_log(&self, record: &OtlpLogRecord) -> Option<AgentEvent> {
        if !is_cursor_cli_resource(&record.resource_attributes) {
            return None;
        }

        let event_name = cursor_log_event_name(record)?;
        let kind = kind_for_cursor_log(&event_name);
        let mut event = AgentEvent::new(self.name(), kind, event_name.clone());
        event.timestamp = record.timestamp;
        event.agent_version = string_attr(&record.resource_attributes, "service.version");
        event.session_id = string_attr(&record.attributes, "cursor.conversation.id");
        event.model = string_attr(&record.attributes, "cursor.model.name");
        event.input_tokens = u64_attr(&record.attributes, "cursor.api.request.input_tokens");
        event.output_tokens = u64_attr(&record.attributes, "cursor.api.request.output_tokens");
        event.status = string_attr(&record.attributes, "cursor.api.status").or_else(|| {
            event_name
                .ends_with("api.error")
                .then(|| "error".to_owned())
        });
        event.attributes = Value::Object(record.attributes.clone());
        event.raw = json!({
            "signal": "log",
            "resource": record.resource_attributes,
            "body": record.body,
        });
        Some(event)
    }

    fn normalize_span(&self, _record: &OtlpSpanRecord) -> Option<AgentEvent> {
        None
    }
}

fn kind_for_hook(name: &str) -> AgentEventKind {
    match name {
        "beforeSubmitPrompt" => AgentEventKind::Observation,
        "preToolUse" => AgentEventKind::Decision,
        "postToolUse" | "postToolUseFailure" => AgentEventKind::ToolCall,
        "subagentStart" | "afterFileEdit" => AgentEventKind::Action,
        "subagentStop" | "afterAgentResponse" => AgentEventKind::Outcome,
        "afterAgentThought" | "sessionStart" | "sessionEnd" | "stop" => AgentEventKind::Trace,
        "preCompact" => AgentEventKind::Log,
        _ => AgentEventKind::Log,
    }
}

fn status_for_hook(attributes: &Map<String, Value>, hook_name: &str) -> Option<String> {
    match hook_name {
        "postToolUse" => Some("success".into()),
        "postToolUseFailure" => string_attr(attributes, "failure_type").or_else(|| Some("error".into())),
        "stop" => string_attr(attributes, "status"),
        "subagentStop" => string_attr(attributes, "status").or_else(|| Some("success".into())),
        _ => string_attr(attributes, "status"),
    }
}

fn safe_hook_attributes(attributes: &Map<String, Value>) -> Map<String, Value> {
    const SAFE_KEYS: &[&str] = &[
        "hook_event_name",
        "cursor_version",
        "conversation_id",
        "generation_id",
        "model",
        "model_id",
        "model_params",
        "tool_name",
        "tool_use_id",
        "duration",
        "duration_ms",
        "failure_type",
        "is_interrupt",
        "subagent_id",
        "subagent_type",
        "parent_conversation_id",
        "tool_call_id",
        "subagent_model",
        "is_parallel_worker",
        "loop_count",
        "status",
        "sandbox",
        "mcp_server_name",
    ];

    SAFE_KEYS
        .iter()
        .filter_map(|key| attributes.get(*key).map(|value| ((*key).to_owned(), value.clone())))
        .collect()
}

fn stable_hook_id(attributes: &Map<String, Value>, hook_name: &str) -> String {
    let key = format!(
        "cursor-hook|{}|{}|{}|{}|{}",
        string_attr(attributes, "conversation_id").unwrap_or_default(),
        string_attr(attributes, "generation_id").unwrap_or_default(),
        hook_name,
        string_attr(attributes, "tool_use_id")
            .or_else(|| string_attr(attributes, "subagent_id"))
            .unwrap_or_default(),
        string_attr(attributes, "tool_name").unwrap_or_default(),
    );
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()).to_string()
}

fn is_cursor_cli_resource(attributes: &Map<String, Value>) -> bool {
    string_attr(attributes, "service.name").as_deref() == Some("cursor")
        && (string_attr(attributes, "cursor.surface").as_deref() == Some("cli")
            || string_attr(attributes, "cursor.entrypoint").as_deref() == Some("cli"))
}

fn cursor_log_event_name(record: &OtlpLogRecord) -> Option<String> {
    if record.event_name.starts_with("cursor.") {
        return Some(record.event_name.clone());
    }
    if let Some(name) = string_attr(&record.attributes, "event.name") {
        if name.starts_with("cursor.") {
            return Some(name);
        }
    }

    record.body.as_str().and_then(|body| match body {
        "api_request" => Some("cursor.api.request".into()),
        "api_error" => Some("cursor.api.error".into()),
        "skill_activated" => Some("cursor.skill.activated".into()),
        "hook_execution_complete" => Some("cursor.hook.execution_complete".into()),
        "plugin_installed" => Some("cursor.plugin.installed".into()),
        value if value.starts_with("api_correction_") => Some("cursor.api.correction".into()),
        _ => None,
    })
}

fn kind_for_cursor_log(name: &str) -> AgentEventKind {
    match name {
        "cursor.api.request" => AgentEventKind::LlmCall,
        "cursor.api.error" => AgentEventKind::Error,
        "cursor.hook.execution_complete" => AgentEventKind::ToolCall,
        "cursor.skill.activated" => AgentEventKind::Action,
        _ => AgentEventKind::Log,
    }
}

fn string_attr(attributes: &Map<String, Value>, key: &str) -> Option<String> {
    attributes.get(key).and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    })
}

fn number_attr(attributes: &Map<String, Value>, key: &str) -> Option<f64> {
    attributes.get(key).and_then(|value| match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    })
}

fn u64_attr(attributes: &Map<String, Value>, key: &str) -> Option<u64> {
    attributes.get(key).and_then(|value| match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::{Map, Value, json};

    use super::{CursorAgentAdapter, SemanticAdapter};
    use crate::{
        core::model::AgentEventKind,
        ingest::otlp::OtlpLogRecord,
    };

    #[test]
    fn normalizes_cursor_hook_without_content() {
        let payload = json!({
            "hook_event_name": "preToolUse",
            "conversation_id": "conv-1",
            "generation_id": "gen-1",
            "cursor_version": "1.7.2",
            "model_id": "gpt-5",
            "tool_name": "Shell",
            "tool_use_id": "tool-1",
            "tool_input": { "command": "cat .env" },
            "agent_message": "Reading secrets",
            "user_email": "person@example.com"
        });

        let event = CursorAgentAdapter.normalize_hook(&payload).unwrap();

        assert_eq!(event.kind, AgentEventKind::Decision);
        assert_eq!(event.session_id.as_deref(), Some("conv-1"));
        assert_eq!(event.turn_id.as_deref(), Some("gen-1"));
        assert_eq!(event.tool_name.as_deref(), Some("Shell"));
        assert!(event.attributes.get("tool_input").is_none());
        assert!(event.attributes.get("agent_message").is_none());
        assert!(event.attributes.get("user_email").is_none());
        assert_eq!(event.raw["content_redacted"], json!(true));
    }

    #[test]
    fn stable_hook_id_deduplicates_retries() {
        let payload = json!({
            "hook_event_name": "postToolUse",
            "conversation_id": "conv-1",
            "generation_id": "gen-1",
            "tool_name": "Shell",
            "tool_use_id": "tool-1",
            "tool_output": "first copy"
        });

        let first = CursorAgentAdapter.normalize_hook(&payload).unwrap();
        let second = CursorAgentAdapter.normalize_hook(&payload).unwrap();
        assert_eq!(first.id, second.id);
    }

    #[test]
    fn normalizes_enterprise_cursor_cli_log() {
        let mut attributes = Map::new();
        attributes.insert("cursor.conversation.id".into(), json!("conv-1"));
        attributes.insert("cursor.model.name".into(), json!("gpt-5"));
        attributes.insert("cursor.api.request.input_tokens".into(), json!(100));
        attributes.insert("cursor.api.request.output_tokens".into(), json!(20));

        let mut resource = Map::new();
        resource.insert("service.name".into(), json!("cursor"));
        resource.insert("cursor.surface".into(), json!("cli"));
        resource.insert("service.version".into(), json!("1.7.2"));

        let event = CursorAgentAdapter
            .normalize_log(&OtlpLogRecord {
                event_name: "otel.log".into(),
                timestamp: Utc::now(),
                trace_id: None,
                span_id: None,
                attributes,
                resource_attributes: resource,
                body: Value::String("api_request".into()),
            })
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::LlmCall);
        assert_eq!(event.session_id.as_deref(), Some("conv-1"));
        assert_eq!(event.input_tokens, Some(100));
        assert_eq!(event.output_tokens, Some(20));
    }

    #[test]
    fn ignores_cursor_desktop_otel() {
        let mut resource = Map::new();
        resource.insert("service.name".into(), json!("cursor"));
        resource.insert("cursor.surface".into(), json!("desktop"));

        assert!(
            CursorAgentAdapter
                .normalize_log(&OtlpLogRecord {
                    event_name: "cursor.api.request".into(),
                    timestamp: Utc::now(),
                    trace_id: None,
                    span_id: None,
                    attributes: Map::new(),
                    resource_attributes: resource,
                    body: Value::Null,
                })
                .is_none()
        );
    }
}
