use serde_json::{Map, Value, json};

use crate::{
    adapters::agents::SemanticAdapter,
    core::model::{AgentEvent, AgentEventKind, DecisionContext},
    ingest::otlp::{OtlpLogRecord, OtlpSpanRecord},
};

#[derive(Clone, Copy, Default)]
pub struct GeminiCliAdapter;

impl SemanticAdapter for GeminiCliAdapter {
    fn name(&self) -> &'static str {
        "gemini-cli"
    }

    fn normalize_log(&self, record: &OtlpLogRecord) -> Option<AgentEvent> {
        let event_name = gemini_event_name(record)?;
        let short_name = event_name
            .strip_prefix("gemini_cli.")
            .unwrap_or(&event_name);
        let kind = kind_for_log(short_name);
        let is_decision = matches!(kind, AgentEventKind::Decision);
        let safe_attributes = redact_sensitive_attributes(&record.attributes);

        let mut event = AgentEvent::new(self.name(), kind, short_name);
        event.timestamp = record.timestamp;
        event.agent_version = string_attr(&record.resource_attributes, "service.version");
        event.session_id = session_id(&record.attributes, &record.resource_attributes);
        event.turn_id = string_attr(&record.attributes, "prompt_id");
        event.trace_id = record.trace_id.clone();
        event.span_id = record.span_id.clone();
        event.model = string_attr_any(
            &record.attributes,
            &[
                "model",
                "model_name",
                "decision_model",
                "gen_ai.request.model",
            ],
        );
        event.tool_name = string_attr_any(
            &record.attributes,
            &["function_name", "tool_name", "gen_ai.tool.name"],
        );
        event.duration_ms = number_attr(&record.attributes, "duration_ms")
            .or_else(|| number_attr(&record.attributes, "duration"));
        event.input_tokens = u64_attr_any(
            &record.attributes,
            &["input_token_count", "gen_ai.usage.input_tokens"],
        );
        event.output_tokens = u64_attr_any(
            &record.attributes,
            &["output_token_count", "gen_ai.usage.output_tokens"],
        );
        event.status = status_for_log(short_name, &record.attributes);

        if is_decision {
            event.decision = decision_context(short_name, &safe_attributes);
        }

        event.attributes = Value::Object(safe_attributes.clone());
        event.raw = json!({
            "signal": "log",
            "event_name": event_name,
            "resource": redact_sensitive_attributes(&record.resource_attributes),
            "body_redacted": true,
        });
        Some(event)
    }

    fn normalize_span(&self, record: &OtlpSpanRecord) -> Option<AgentEvent> {
        if !is_gemini_span(record) {
            return None;
        }

        let operation = string_attr(&record.attributes, "gen_ai.operation.name")
            .unwrap_or_else(|| record.name.clone());
        let kind = kind_for_operation(&operation);
        let mut event = AgentEvent::new(self.name(), kind, operation.clone());
        event.timestamp = record.timestamp;
        event.agent_version = string_attr(&record.resource_attributes, "service.version");
        event.session_id = string_attr(&record.attributes, "gen_ai.conversation.id")
            .or_else(|| session_id(&record.attributes, &record.resource_attributes));
        event.turn_id = string_attr(&record.attributes, "prompt_id");
        event.trace_id = Some(record.trace_id.clone());
        event.span_id = Some(record.span_id.clone());
        event.model = string_attr_any(
            &record.attributes,
            &["gen_ai.request.model", "gen_ai.response.model", "model"],
        );
        event.tool_name = string_attr(&record.attributes, "gen_ai.tool.name");
        event.duration_ms = Some(record.duration_ms);
        event.input_tokens = u64_attr(&record.attributes, "gen_ai.usage.input_tokens");
        event.output_tokens = u64_attr(&record.attributes, "gen_ai.usage.output_tokens");
        event.status = record.status.clone();
        event.attributes = Value::Object(redact_sensitive_attributes(&record.attributes));
        event.raw = json!({
            "signal": "trace",
            "span_name": record.name,
            "parent_span_id": record.parent_span_id,
            "resource": redact_sensitive_attributes(&record.resource_attributes),
        });
        Some(event)
    }
}

fn gemini_event_name(record: &OtlpLogRecord) -> Option<String> {
    if record.event_name.starts_with("gemini_cli.") {
        return Some(record.event_name.clone());
    }
    if let Some(name) = string_attr(&record.attributes, "event.name")
        && name.starts_with("gemini_cli.")
    {
        return Some(name);
    }
    None
}

fn is_gemini_span(record: &OtlpSpanRecord) -> bool {
    string_attr(&record.attributes, "gen_ai.agent.name").as_deref() == Some("gemini-cli")
        || string_attr(&record.resource_attributes, "service.name")
            .map(|name| name.contains("gemini"))
            .unwrap_or(false)
}

fn kind_for_log(name: &str) -> AgentEventKind {
    match name {
        "user_prompt" => AgentEventKind::Observation,
        "tool_call" | "hook_call" => AgentEventKind::ToolCall,
        "api_request" | "api_response" => AgentEventKind::LlmCall,
        "api_error"
        | "malformed_json_response"
        | "chat.invalid_chunk"
        | "chat.content_retry_failure" => AgentEventKind::Error,
        "model_routing" | "conseca.verdict" => AgentEventKind::Decision,
        "file_operation" | "slash_command" | "plan_execution" => AgentEventKind::Action,
        "conversation_finished" | "agent.finish" => AgentEventKind::Outcome,
        _ => AgentEventKind::Log,
    }
}

fn kind_for_operation(operation: &str) -> AgentEventKind {
    match operation {
        "user_prompt" | "system_prompt" => AgentEventKind::Observation,
        "llm_call" | "chat" | "generate_content" => AgentEventKind::LlmCall,
        "tool_call" | "schedule_tool_calls" | "execute_tool" => AgentEventKind::ToolCall,
        "agent_call" => AgentEventKind::Trace,
        _ => AgentEventKind::Trace,
    }
}

fn decision_context(name: &str, attributes: &Map<String, Value>) -> Option<DecisionContext> {
    match name {
        "model_routing" => Some(DecisionContext {
            question: Some("Which model should Gemini CLI route this request to?".into()),
            evidence: string_attr(attributes, "decision_source")
                .into_iter()
                .collect(),
            alternatives: Vec::new(),
            selected: string_attr(attributes, "decision_model"),
            constraints: string_attr(attributes, "approval_mode")
                .into_iter()
                .collect(),
            assumptions: Vec::new(),
            confidence: None,
            expected_outcome: None,
            ..Default::default()
        }),
        "conseca.verdict" => Some(DecisionContext {
            question: string_attr(attributes, "tool_name")
                .map(|tool| format!("Allow Gemini CLI tool `{tool}`?")),
            evidence: Vec::new(),
            alternatives: vec!["accept".into(), "reject".into(), "modify".into()],
            selected: string_attr(attributes, "decision"),
            constraints: string_attr(attributes, "verdict").into_iter().collect(),
            assumptions: Vec::new(),
            confidence: None,
            expected_outcome: None,
            ..Default::default()
        }),
        _ => None,
    }
}

fn status_for_log(name: &str, attributes: &Map<String, Value>) -> Option<String> {
    if let Some(status) = string_attr(attributes, "status") {
        return Some(status);
    }

    if let Some(failed) = attributes.get("failed") {
        match failed {
            Value::Bool(true) => return Some("error".into()),
            Value::Bool(false) => return Some("success".into()),
            Value::String(value) if value == "true" => return Some("error".into()),
            Value::String(value) if value == "false" => return Some("success".into()),
            _ => {}
        }
    }

    if let Some(success) = attributes.get("success") {
        match success {
            Value::Bool(true) => return Some("success".into()),
            Value::Bool(false) => return Some("error".into()),
            Value::String(value) if value == "true" => return Some("success".into()),
            Value::String(value) if value == "false" => return Some("error".into()),
            _ => {}
        }
    }

    if name == "api_error"
        || attributes.contains_key("error.message")
        || attributes.contains_key("error_message")
    {
        return Some("error".into());
    }

    attributes
        .get("status_code")
        .and_then(|value| match value {
            Value::Number(value) => value.as_u64(),
            Value::String(value) => value.parse().ok(),
            _ => None,
        })
        .map(|status| if status >= 400 { "error" } else { "success" }.into())
}

fn session_id(
    attributes: &Map<String, Value>,
    resource_attributes: &Map<String, Value>,
) -> Option<String> {
    string_attr(attributes, "session.id")
        .or_else(|| string_attr(resource_attributes, "session.id"))
        .or_else(|| string_attr(attributes, "gen_ai.conversation.id"))
}

fn redact_sensitive_attributes(attributes: &Map<String, Value>) -> Map<String, Value> {
    const REDACTED_KEYS: &[&str] = &[
        "user.email",
        "prompt",
        "request_text",
        "response_text",
        "function_args",
        "gen_ai.input.messages",
        "gen_ai.output.messages",
        "gen_ai.system_instructions",
        "gen_ai.tool.definitions",
        "gen_ai.tool.description",
        "gen_ai.tool.call.arguments",
        "reason",
        "reasoning",
        "error.message",
        "error_message",
    ];

    attributes
        .iter()
        .filter(|(key, _)| !REDACTED_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn string_attr(attributes: &Map<String, Value>, key: &str) -> Option<String> {
    attributes.get(key).and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    })
}

fn string_attr_any(attributes: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| string_attr(attributes, key))
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

fn u64_attr_any(attributes: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| u64_attr(attributes, key))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::{Map, Value, json};

    use super::{GeminiCliAdapter, SemanticAdapter};
    use crate::{
        core::model::AgentEventKind,
        ingest::otlp::{OtlpLogRecord, OtlpSpanRecord},
    };

    fn log_record(event_name: &str, attributes: Map<String, Value>) -> OtlpLogRecord {
        OtlpLogRecord {
            event_name: event_name.into(),
            timestamp: Utc::now(),
            trace_id: None,
            span_id: None,
            attributes,
            resource_attributes: Map::new(),
            body: Value::Null,
        }
    }

    #[test]
    fn normalizes_user_prompt_without_prompt_content() {
        let mut attributes = Map::new();
        attributes.insert("session.id".into(), json!("session-1"));
        attributes.insert("prompt_id".into(), json!("prompt-1"));
        attributes.insert("prompt_length".into(), json!(42));
        attributes.insert("prompt".into(), json!("secret prompt"));
        attributes.insert("user.email".into(), json!("person@example.com"));

        let event = GeminiCliAdapter
            .normalize_log(&log_record("gemini_cli.user_prompt", attributes))
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::Observation);
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.turn_id.as_deref(), Some("prompt-1"));
        assert!(event.attributes.get("prompt").is_none());
        assert!(event.attributes.get("user.email").is_none());
    }

    #[test]
    fn normalizes_tool_call() {
        let mut attributes = Map::new();
        attributes.insert("session.id".into(), json!("session-1"));
        attributes.insert("prompt_id".into(), json!("prompt-1"));
        attributes.insert("function_name".into(), json!("run_shell_command"));
        attributes.insert("duration_ms".into(), json!(15));
        attributes.insert("success".into(), json!("true"));
        attributes.insert("decision".into(), json!("accept"));
        attributes.insert("function_args".into(), json!("{\"command\":\"cat .env\"}"));

        let event = GeminiCliAdapter
            .normalize_log(&log_record("gemini_cli.tool_call", attributes))
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::ToolCall);
        assert_eq!(event.tool_name.as_deref(), Some("run_shell_command"));
        assert_eq!(event.status.as_deref(), Some("success"));
        assert_eq!(event.duration_ms, Some(15.0));
        assert!(event.attributes.get("function_args").is_none());
    }

    #[test]
    fn normalizes_model_routing_failure_without_error_text() {
        let mut attributes = Map::new();
        attributes.insert("decision_model".into(), json!("gemini-2.5-flash"));
        attributes.insert("decision_source".into(), json!("fallback"));
        attributes.insert("reasoning".into(), json!("quota exhausted"));
        attributes.insert("error_message".into(), json!("private backend detail"));
        attributes.insert("failed".into(), json!(true));

        let event = GeminiCliAdapter
            .normalize_log(&log_record("gemini_cli.model_routing", attributes))
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::Decision);
        assert_eq!(event.model.as_deref(), Some("gemini-2.5-flash"));
        assert_eq!(event.status.as_deref(), Some("error"));
        assert_eq!(
            event.decision.unwrap().selected.as_deref(),
            Some("gemini-2.5-flash")
        );
        assert!(event.attributes.get("reasoning").is_none());
        assert!(event.attributes.get("error_message").is_none());
    }

    #[test]
    fn conseca_reason_does_not_leak_into_decision() {
        let mut attributes = Map::new();
        attributes.insert("tool_name".into(), json!("run_shell_command"));
        attributes.insert("decision".into(), json!("reject"));
        attributes.insert("verdict".into(), json!("blocked"));
        attributes.insert("reason".into(), json!("sensitive policy rationale"));

        let event = GeminiCliAdapter
            .normalize_log(&log_record("gemini_cli.conseca.verdict", attributes))
            .unwrap();

        let decision = event.decision.unwrap();
        assert!(decision.evidence.is_empty());
        assert!(event.attributes.get("reason").is_none());
    }

    #[test]
    fn normalizes_standard_genai_operations_and_redacts_tool_arguments() {
        let mut attributes = Map::new();
        attributes.insert("gen_ai.agent.name".into(), json!("gemini-cli"));
        attributes.insert("gen_ai.operation.name".into(), json!("execute_tool"));
        attributes.insert("gen_ai.conversation.id".into(), json!("session-1"));
        attributes.insert("gen_ai.tool.name".into(), json!("run_shell_command"));
        attributes.insert(
            "gen_ai.tool.call.arguments".into(),
            json!("{\"command\":\"cat .env\"}"),
        );

        let event = GeminiCliAdapter
            .normalize_span(&OtlpSpanRecord {
                name: "execute_tool run_shell_command".into(),
                timestamp: Utc::now(),
                trace_id: "trace-1".into(),
                span_id: "span-1".into(),
                parent_span_id: None,
                duration_ms: 12.0,
                status: Some("success".into()),
                attributes,
                resource_attributes: Map::new(),
            })
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::ToolCall);
        assert!(event.attributes.get("gen_ai.tool.call.arguments").is_none());
    }

    #[test]
    fn normalizes_genai_trace() {
        let mut attributes = Map::new();
        attributes.insert("gen_ai.agent.name".into(), json!("gemini-cli"));
        attributes.insert("gen_ai.operation.name".into(), json!("generate_content"));
        attributes.insert("gen_ai.conversation.id".into(), json!("session-1"));
        attributes.insert("gen_ai.request.model".into(), json!("gemini-2.5-pro"));
        attributes.insert("gen_ai.usage.input_tokens".into(), json!(100));
        attributes.insert("gen_ai.usage.output_tokens".into(), json!(20));
        attributes.insert("gen_ai.input.messages".into(), json!("secret input"));

        let event = GeminiCliAdapter
            .normalize_span(&OtlpSpanRecord {
                name: "generate_content".into(),
                timestamp: Utc::now(),
                trace_id: "trace-1".into(),
                span_id: "span-1".into(),
                parent_span_id: None,
                duration_ms: 123.0,
                status: Some("success".into()),
                attributes,
                resource_attributes: Map::new(),
            })
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::LlmCall);
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(event.input_tokens, Some(100));
        assert_eq!(event.output_tokens, Some(20));
        assert!(event.attributes.get("gen_ai.input.messages").is_none());
    }

    #[test]
    fn ignores_non_gemini_span() {
        assert!(
            GeminiCliAdapter
                .normalize_span(&OtlpSpanRecord {
                    name: "http.request".into(),
                    timestamp: Utc::now(),
                    trace_id: "trace-1".into(),
                    span_id: "span-1".into(),
                    parent_span_id: None,
                    duration_ms: 12.0,
                    status: None,
                    attributes: Map::new(),
                    resource_attributes: Map::new(),
                })
                .is_none()
        );
    }
}
