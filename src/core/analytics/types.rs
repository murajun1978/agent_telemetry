use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::model::{AgentEvent, AgentEventKind};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: f64,
}

impl TokenTotals {
    pub(super) fn add_event(&mut self, event: &AgentEvent) {
        let usage = event.effective_token_usage();
        self.input_tokens += usage.input_tokens.unwrap_or(0);
        self.output_tokens += usage.output_tokens.unwrap_or(0);
        self.total_tokens += usage.total_tokens();
        self.cached_input_tokens += usage.cached_input_tokens.unwrap_or(0);
        self.reasoning_tokens += usage.reasoning_tokens.unwrap_or(0);
        self.cost_usd += usage.cost_usd.unwrap_or(0.0);
    }

    pub(super) fn add_totals(&mut self, other: &Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.cost_usd += other.cost_usd;
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TokenEfficiency {
    pub tokens_per_success: Option<f64>,
    pub cost_per_success: Option<f64>,
    pub tokens_per_decision: Option<f64>,
    pub tokens_per_tool_call: Option<f64>,
    pub retry_token_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub kind: AgentEventKind,
    pub name: String,
    pub status: Option<String>,
    pub tool_name: Option<String>,
    pub model: Option<String>,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnAnalytics {
    pub turn_id: Option<String>,
    pub tokens: TokenTotals,
    pub decisions: usize,
    pub tool_calls: usize,
    pub actions: usize,
    pub outcomes: usize,
    pub successful_outcomes: usize,
    pub errors: usize,
    pub retries: usize,
    pub efficiency: TokenEfficiency,
    pub flow: Vec<FlowEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionAnalytics {
    pub agent: String,
    pub session_id: Option<String>,
    pub tokens: TokenTotals,
    pub decisions: usize,
    pub tool_calls: usize,
    pub actions: usize,
    pub outcomes: usize,
    pub successful_outcomes: usize,
    pub errors: usize,
    pub retries: usize,
    pub efficiency: TokenEfficiency,
    pub by_model: BTreeMap<String, TokenTotals>,
    pub turns: Vec<TurnAnalytics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionComparison {
    pub agent: String,
    pub session_id: String,
    pub successful: Option<bool>,
    pub tokens: TokenTotals,
    pub decisions: usize,
    pub tool_calls: usize,
    pub actions: usize,
    pub outcomes: usize,
    pub errors: usize,
    pub retries: usize,
    pub efficiency: TokenEfficiency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentComparison {
    pub agent: String,
    pub sessions: usize,
    pub outcome_observed_sessions: usize,
    pub successful_sessions: usize,
    pub session_success_rate: Option<f64>,
    pub tokens: TokenTotals,
    pub decisions: usize,
    pub tool_calls: usize,
    pub actions: usize,
    pub outcomes: usize,
    pub errors: usize,
    pub retries: usize,
    pub average_tokens_per_session: Option<f64>,
    pub average_cost_per_session: Option<f64>,
    pub tokens_per_successful_session: Option<f64>,
    pub cost_per_successful_session: Option<f64>,
    pub efficiency: TokenEfficiency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelComparison {
    pub model: String,
    pub agents: Vec<String>,
    pub events: usize,
    pub tokens: TokenTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComparisonReport {
    pub events: usize,
    pub unscoped_events: usize,
    pub sessions: usize,
    pub agents: Vec<AgentComparison>,
    pub models: Vec<ModelComparison>,
    pub session_details: Vec<SessionComparison>,
}
