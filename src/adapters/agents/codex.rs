use serde_json::{Map, Value, json};

use crate::{
    adapters::agents::SemanticAdapter,
    core::model::{AgentEvent, AgentEventKind, DecisionContext},
    ingest::otlp::{OtlpLogRecord, OtlpSpanRecord},
};

pub struct CodexAdapter;

impl SemanticAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn normalize_log(&self, record: &OtlpLogRecord) -> Option<AgentEvent> {
        let event_name = codex_event_name(record)?;
        let short_name = event_name.strip_prefix("codex.").unwrap_or(&event_name);
        let kind = kind_for_event(short_name);

        let mut canonical = AgentEvent::new(self.name(), kind, short_name);
        populate_common_log_fields(&mut canonical, record);

        if matches!(kind, AgentEventKind::Decision) {
            canonical.decision = decision_context(short_name, &record.attributes);
        }

        canonical.attributes = Value::Object(record.attributes.clone());
        canonical.raw = json!({
            "signal": "log",
            "otlp_event_name": record.event_name,
            "event_name": event_name,
            "resource": record.resource_attributes,
            "body": record.body,
        });
        Some(canonical)
    }

    fn normalize_span(&self, record: &OtlpSpanRecord) -> Option<AgentEvent> {
        let event_name = string_attr(&record.attributes, "event.name")
            .filter(|name| name.starts_with("codex."));
        let is_codex_span = event_name.is_some()
            || record.name.starts_with("codex.")
            || codex_service(&record.resource_attributes)
            || record.attributes.contains_key("thread.id")
            || record.attributes.contains_key("conversation.id");

        if !is_codex_span {
            return None;
        }

        let canonical_name = event_name
            .as_deref()
            .unwrap_or(&record.name)
            .strip_prefix("codex.")
            .unwrap_or(event_name.as_deref().unwrap_or(&record.name));
        let kind = if event_name.is_some() {
            kind_for_event(canonical_name)
        } else {
            AgentEventKind::Trace
        };

        let mut canonical = AgentEvent::new(self.name(), kind, canonical_name);
        canonical.timestamp = record.timestamp;
        canonical.agent_version = string_attr_any(
            &record.attributes,
            &["app.version", "service.version"],
        )
        .or_else(|| string_attr(&record.resource_attributes, "service.version"));
        canonical.session_id = string_attr_any(
            &record.attributes,
            &["conversation.id", "thread.id", "session.id"],
        );
        canonical.turn_id = string_attr_any(
            &record.attributes,
            &["turn_id", "turn.id", "prompt.id"],
        );
        canonical.trace_id = Some(record.trace_id.clone());
        canonical.span_id = Some(record.span_id.clone());
        canonical.model = string_attr_any(
            &record.attributes,
            &["model", "gen_ai.request.model"],
        );
        canonical.tool_name = string_attr_any(
            &record.attributes,
            &["tool_name", "gen_ai.tool.name"],
        );
        canonical.duration_ms = number_attr(&record.attributes, "duration_ms")
            .or(Some(record.duration_ms));
        canonical.input_tokens = u64_attr_any(
            &record.attributes,
            &[
                "input_token_count",
                "input_tokens",
                "codex.turn.token_usage.input_tokens",
                "gen_ai.usage.input_tokens",
            ],
        );
        canonical.output_tokens = u64_attr_any(
            &record.attributes,
            &[
                "output_token_count",
                "output_tokens",
                "codex.turn.token_usage.output_tokens",
                "gen_ai.usage.output_tokens",
            ],
        );
        canonical.cost_usd = cost_usd(&record.attributes);
        canonical.status = record
            .status
            .clone()
            .or_else(|| status_from_attributes(&record.attributes));

        if matches!(kind, AgentEventKind::Decision) {
            canonical.decision = decision_context(canonical_name, &record.attributes);
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

fn codex_event_name(record: &OtlpLogRecord) -> Option<String> {
    string_attr(&record.attributes, "event.name")
        .filter(|name| name.starts_with("codex."))
        .or_else(|| record.event_name.starts_with("codex.").then(|| record.event_name.clone()))
}

fn kind_for_event(name: &str) -> AgentEventKind {
    match name {
        "user_prompt" => AgentEventKind::Observation,
        "tool_decision" | "network_proxy.policy_decision" => AgentEventKind::Decision,
        "tool_result" | "tool_call" | "tool_call_received" | "tool_result_ready" => {
            AgentEventKind::ToolCall
        }
        "api_request" | "sse_event" | "websocket.request" | "websocket.event" => {
            AgentEventKind::LlmCall
        }
        "agent_communication" => AgentEventKind::Action,
        _ => AgentEventKind::Log,
    }
}

fn populate_common_log_fields(event: &mut AgentEvent, record: &OtlpLogRecord) {
    event.timestamp = record.timestamp;
    event.agent_version = string_attr(&record.attributes, "app.version")
        .or_else(|| string_attr(&record.resource_attributes, "service.version"));
    event.session_id = string_attr_any(
        &record.attributes,
        &["conversation.id", "thread.id", "session.id"],
    );
    event.turn_id = string_attr_any(
        &record.attributes,
        &["turn_id", "turn.id", "prompt.id"],
    );
    event.trace_id = record.trace_id.clone();
    event.span_id = record.span_id.clone();
    event.model = string_attr_any(
        &record.attributes,
        &["model", "gen_ai.request.model"],
    );
    event.tool_name = string_attr_any(
        &record.attributes,
        &["tool_name", "gen_ai.tool.name"],
    );
    event.duration_ms = number_attr(&record.attributes, "duration_ms");
    event.input_tokens = u64_attr_any(
        &record.attributes,
        &[
            "input_token_count",
            "input_tokens",
            "codex.turn.token_usage.input_tokens",
            "gen_ai.usage.input_tokens",
        ],
    );
    event.output_tokens = u64_attr_any(
        &record.attributes,
        &[
            "output_token_count",
            "output_tokens",
            "codex.turn.token_usage.output_tokens",
            "gen_ai.usage.output_tokens",
        ],
    );
    event.cost_usd = cost_usd(&record.attributes);
    event.status = status_from_attributes(&record.attributes);
}

fn decision_context(name: &str, attributes: &Map<String, Value>) -> Option<DecisionContext> {
    let selected = string_attr_any(attributes, &["decision", "decision_type"]);
    let source = string_attr_any(attributes, &["source", "decision_source"]);

    if selected.is_none() && source.is_none() && name != "tool_decision" {
        return None;
    }

    let question = match name {
        "tool_decision" => string_attr(attributes, "tool_name")
            .map(|tool| format!("Allow tool `{tool}` to execute?")),
        "network_proxy.policy_decision" => Some("Allow the requested network access?".into()),
        _ => None,
    };

    Some(DecisionContext {
        question,
        evidence: source.into_iter().collect(),
        alternatives: Vec::new(),
        selected,
        constraints: Vec::new(),
        assumptions: Vec::new(),
        confidence: None,
        expected_outcome: None,
    })
}

fn cost_usd(attributes: &Map<String, Value>) -> Option<f64> {
    number_attr(attributes, "cost_usd")
        .or_else(|| {
            u64_attr(attributes, "cost_usd_micros")
                .map(|micros| micros as f64 / 1_000_000.0)
        })
        .or_else(|| {
            u64_attr(attributes, "codex.turn.cost_microusd")
                .map(|micros| micros as f64 / 1_000_000.0)
        })
}

fn status_from_attributes(attributes: &Map<String, Value>) -> Option<String> {
    if let Some(success) = attributes.get("success") {
        match success {
            Value::Bool(true) => return Some("success".into()),
            Value::Bool(false) => return Some("error".into()),
            Value::String(value) if value == "true" => return Some("success".into()),
            Value::String(value) if value == "false" => return Some("error".into()),
            _ => {}
        }
    }

    if string_attr(attributes, "error.message").is_some() {
        return Some("error".into());
    }

    attributes
        .get("http.response.status_code")
        .and_then(|value| match value {
            Value::Number(value) => value.as_u64(),
            Value::String(value) => value.parse().ok(),
            _ => None,
        })
        .map(|status| if status >= 400 { "error" } else { "success" }.into())
}

fn codex_service(attributes: &Map<String, Value>) -> bool {
    string_attr(attributes, "service.name")
        .map(|name| name.to_ascii_lowercase().contains("codex"))
        .unwrap_or(false)
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

    use super::{CodexAdapter, SemanticAdapter};
    use crate::{
        core::model::AgentEventKind,
        ingest::otlp::{OtlpLogRecord, OtlpSpanRecord},
    };

    fn log_record(event_name: &str, attributes: Map<String, Value>) -> OtlpLogRecord {
        OtlpLogRecord {
            event_name: event_name.into(),
            timestamp: Utc::now(),
            trace_id: Some("trace-1".into()),
            span_id: Some("span-1".into()),
            attributes,
            resource_attributes: Map::new(),
            body: Value::Null,
        }
    }

    #[test]
    fn prefers_codex_event_name_attribute() {
        let mut attributes = Map::new();
        attributes.insert("event.name".into(), json!("codex.user_prompt"));
        attributes.insert("conversation.id".into(), json!("conversation-1"));
        attributes.insert("model".into(), json!("gpt-5"));

        let event = CodexAdapter
            .normalize_log(&log_record("event session_telemetry.rs:123", attributes))
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::Observation);
        assert_eq!(event.name, "user_prompt");
        assert_eq!(event.session_id.as_deref(), Some("conversation-1"));
        assert_eq!(event.model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn normalizes_tool_decision() {
        let mut attributes = Map::new();
        attributes.insert("event.name".into(), json!("codex.tool_decision"));
        attributes.insert("tool_name".into(), json!("shell"));
        attributes.insert("decision".into(), json!("approved"));
        attributes.insert("source".into(), json!("user"));

        let event = CodexAdapter
            .normalize_log(&log_record("event session_telemetry.rs:456", attributes))
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::Decision);
        let decision = event.decision.unwrap();
        assert_eq!(decision.selected.as_deref(), Some("approved"));
        assert_eq!(decision.evidence, vec!["user"]);
    }

    #[test]
    fn normalizes_tool_result_status_and_duration() {
        let mut attributes = Map::new();
        attributes.insert("event.name".into(), json!("codex.tool_result"));
        attributes.insert("tool_name".into(), json!("shell"));
        attributes.insert("success".into(), json!(true));
        attributes.insert("duration_ms".into(), json!(42));

        let event = CodexAdapter
            .normalize_log(&log_record("event tool_result.rs:10", attributes))
            .unwrap();

        assert_eq!(event.kind, AgentEventKind::ToolCall);
        assert_eq!(event.tool_name.as_deref(), Some("shell"));
        assert_eq!(event.status.as_deref(), Some("success"));
        assert_eq!(event.duration_ms, Some(42.0));
    }

    #[test]
    fn normalizes_codex_turn_span() {
        let mut attributes = Map::new();
        attributes.insert("thread.id".into(), json!("thread-1"));
        attributes.insert("turn.id".into(), json!("turn-1"));
        attributes.insert("model".into(), json!("gpt-5"));
        attributes.insert("input_token_count".into(), json!(100));
        attributes.insert("output_token_count".into(), json!(25));

        let event = CodexAdapter
            .normalize_span(&OtlpSpanRecord {
                name: "session_task.turn".into(),
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

        assert_eq!(event.kind, AgentEventKind::Trace);
        assert_eq!(event.session_id.as_deref(), Some("thread-1"));
        assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(event.input_tokens, Some(100));
        assert_eq!(event.output_tokens, Some(25));
    }
}
