use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventKind {
    Observation,
    Decision,
    Action,
    Outcome,
    Learning,
    LlmCall,
    ToolCall,
    Error,
    Metric,
    Log,
    Trace,
    Unknown,
}

impl AgentEventKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Decision => "decision",
            Self::Action => "action",
            Self::Outcome => "outcome",
            Self::Learning => "learning",
            Self::LlmCall => "llm_call",
            Self::ToolCall => "tool_call",
            Self::Error => "error",
            Self::Metric => "metric",
            Self::Log => "log",
            Self::Trace => "trace",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub breakdown: Value,
}

impl TokenUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.unwrap_or(0) + self.output_tokens.unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cached_input_tokens.is_none()
            && self.reasoning_tokens.is_none()
            && self.cost_usd.is_none()
            && self
                .breakdown
                .as_object()
                .map(|values| values.is_empty())
                .unwrap_or(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionContext {
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub selected: Option<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub expected_outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentEvent {
    #[serde(default = "new_event_id")]
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub agent: String,
    #[serde(default)]
    pub agent_version: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub span_id: Option<String>,
    pub kind: AgentEventKind,
    pub name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
    #[serde(default)]
    pub decision: Option<DecisionContext>,
    #[serde(default)]
    pub attributes: Value,
    #[serde(default)]
    pub raw: Value,
}

fn new_event_id() -> String {
    Uuid::new_v4().to_string()
}

impl AgentEvent {
    pub fn new(agent: impl Into<String>, kind: AgentEventKind, name: impl Into<String>) -> Self {
        Self {
            id: new_event_id(),
            timestamp: Utc::now(),
            agent: agent.into(),
            agent_version: None,
            session_id: None,
            turn_id: None,
            trace_id: None,
            span_id: None,
            kind,
            name: name.into(),
            model: None,
            tool_name: None,
            status: None,
            duration_ms: None,
            input_tokens: None,
            output_tokens: None,
            cost_usd: None,
            token_usage: None,
            decision: None,
            attributes: Value::Object(Default::default()),
            raw: Value::Null,
        }
    }

    pub fn effective_token_usage(&self) -> TokenUsage {
        let mut usage = self.token_usage.clone().unwrap_or_default();
        usage.input_tokens = usage.input_tokens.or(self.input_tokens);
        usage.output_tokens = usage.output_tokens.or(self.output_tokens);
        usage.cost_usd = usage.cost_usd.or(self.cost_usd);

        if let Some(attributes) = self.attributes.as_object() {
            usage.cached_input_tokens = usage.cached_input_tokens.or_else(|| {
                u64_attr_any(
                    attributes,
                    &[
                        "cached_input_tokens",
                        "cache_read_input_tokens",
                        "gen_ai.usage.cached_input_tokens",
                    ],
                )
            });
            usage.reasoning_tokens = usage.reasoning_tokens.or_else(|| {
                u64_attr_any(
                    attributes,
                    &[
                        "reasoning_tokens",
                        "output_reasoning_tokens",
                        "codex.turn.token_usage.reasoning_tokens",
                        "gen_ai.usage.reasoning_tokens",
                    ],
                )
            });

            if usage.breakdown.is_null() {
                usage.breakdown = Value::Object(Map::new());
            }
            if let Some(breakdown) = usage.breakdown.as_object_mut() {
                for key in [
                    "cache_creation_input_tokens",
                    "cache_read_input_tokens",
                    "cached_input_tokens",
                    "reasoning_tokens",
                    "output_reasoning_tokens",
                    "codex.turn.token_usage.reasoning_tokens",
                    "gen_ai.usage.reasoning_tokens",
                ] {
                    if let Some(value) = attributes.get(key)
                        && (value.is_number() || value.is_string())
                    {
                        breakdown.insert(key.to_owned(), value.clone());
                    }
                }
            }
        }

        usage
    }

    pub fn hydrate_token_usage(&mut self) {
        let usage = self.effective_token_usage();
        if !usage.is_empty() {
            self.token_usage = Some(usage);
        }
    }
}

fn u64_attr_any(attributes: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        attributes.get(*key).and_then(|value| match value {
            Value::Number(value) => value.as_u64(),
            Value::String(value) => value.parse().ok(),
            _ => None,
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{AgentEvent, AgentEventKind};

    #[test]
    fn event_kind_str_matches_serde_wire_format() {
        assert_eq!(AgentEventKind::LlmCall.as_str(), "llm_call");
        assert_eq!(AgentEventKind::ToolCall.as_str(), "tool_call");
        assert_eq!(
            serde_json::to_string(&AgentEventKind::LlmCall).unwrap(),
            "\"llm_call\""
        );
    }

    #[test]
    fn hydrates_token_usage_from_legacy_fields_and_attributes() {
        let mut event = AgentEvent::new("codex", AgentEventKind::LlmCall, "api_request");
        event.input_tokens = Some(100);
        event.output_tokens = Some(25);
        event.cost_usd = Some(0.01);
        event.attributes = json!({
            "cached_input_tokens": 40,
            "reasoning_tokens": "5"
        });

        event.hydrate_token_usage();
        let usage = event.token_usage.unwrap();

        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(25));
        assert_eq!(usage.total_tokens(), 125);
        assert_eq!(usage.cached_input_tokens, Some(40));
        assert_eq!(usage.reasoning_tokens, Some(5));
        assert_eq!(usage.cost_usd, Some(0.01));
    }
}
