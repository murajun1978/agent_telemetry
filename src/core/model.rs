use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
            decision: None,
            attributes: Value::Object(Default::default()),
            raw: Value::Null,
        }
    }
}
