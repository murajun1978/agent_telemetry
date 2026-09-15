use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::model::{AgentEvent, AgentEventKind};

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
    fn add_event(&mut self, event: &AgentEvent) {
        let usage = event.effective_token_usage();
        self.input_tokens += usage.input_tokens.unwrap_or(0);
        self.output_tokens += usage.output_tokens.unwrap_or(0);
        self.total_tokens += usage.total_tokens();
        self.cached_input_tokens += usage.cached_input_tokens.unwrap_or(0);
        self.reasoning_tokens += usage.reasoning_tokens.unwrap_or(0);
        self.cost_usd += usage.cost_usd.unwrap_or(0.0);
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

#[derive(Debug, Clone, Default)]
struct Aggregate {
    tokens: TokenTotals,
    decisions: usize,
    tool_calls: usize,
    actions: usize,
    outcomes: usize,
    successful_outcomes: usize,
    errors: usize,
    retries: usize,
    retry_tokens: u64,
}

impl Aggregate {
    fn add_event(&mut self, event: &AgentEvent) {
        let usage = event.effective_token_usage();
        let event_tokens = usage.total_tokens();
        self.tokens.add_event(event);

        match event.kind {
            AgentEventKind::Decision => self.decisions += 1,
            AgentEventKind::ToolCall => self.tool_calls += 1,
            AgentEventKind::Action => self.actions += 1,
            AgentEventKind::Outcome => {
                self.outcomes += 1;
                if is_success(event) {
                    self.successful_outcomes += 1;
                }
            }
            AgentEventKind::Error => self.errors += 1,
            _ => {}
        }

        if is_retry(event) {
            self.retries += 1;
            self.retry_tokens += event_tokens;
        }
    }

    fn efficiency(&self) -> TokenEfficiency {
        TokenEfficiency {
            tokens_per_success: ratio(self.tokens.total_tokens, self.successful_outcomes),
            cost_per_success: float_ratio(self.tokens.cost_usd, self.successful_outcomes),
            tokens_per_decision: ratio(self.tokens.total_tokens, self.decisions),
            tokens_per_tool_call: ratio(self.tokens.total_tokens, self.tool_calls),
            retry_token_ratio: if self.tokens.total_tokens == 0 {
                None
            } else {
                Some(self.retry_tokens as f64 / self.tokens.total_tokens as f64)
            },
        }
    }
}

pub fn analyze_session(events: &[AgentEvent]) -> Option<SessionAnalytics> {
    let first = events.first()?;
    let mut aggregate = Aggregate::default();
    let mut models: BTreeMap<String, TokenTotals> = BTreeMap::new();
    let mut turns: HashMap<Option<String>, Aggregate> = HashMap::new();

    for event in events {
        aggregate.add_event(event);

        if let Some(model) = &event.model {
            models.entry(model.clone()).or_default().add_event(event);
        }

        turns
            .entry(event.turn_id.clone())
            .or_default()
            .add_event(event);
    }

    let mut turn_rows = turns
        .into_iter()
        .map(|(turn_id, values)| TurnAnalytics {
            turn_id,
            tokens: values.tokens.clone(),
            decisions: values.decisions,
            tool_calls: values.tool_calls,
            actions: values.actions,
            outcomes: values.outcomes,
            successful_outcomes: values.successful_outcomes,
            errors: values.errors,
            retries: values.retries,
            efficiency: values.efficiency(),
        })
        .collect::<Vec<_>>();
    turn_rows.sort_by(|a, b| a.turn_id.cmp(&b.turn_id));

    Some(SessionAnalytics {
        agent: first.agent.clone(),
        session_id: first.session_id.clone(),
        tokens: aggregate.tokens.clone(),
        decisions: aggregate.decisions,
        tool_calls: aggregate.tool_calls,
        actions: aggregate.actions,
        outcomes: aggregate.outcomes,
        successful_outcomes: aggregate.successful_outcomes,
        errors: aggregate.errors,
        retries: aggregate.retries,
        efficiency: aggregate.efficiency(),
        by_model: models,
        turns: turn_rows,
    })
}

fn ratio(value: u64, count: usize) -> Option<f64> {
    (count > 0).then(|| value as f64 / count as f64)
}

fn float_ratio(value: f64, count: usize) -> Option<f64> {
    (count > 0).then(|| value / count as f64)
}

fn is_success(event: &AgentEvent) -> bool {
    event.status.as_deref().is_some_and(|status| {
        matches!(status.to_ascii_lowercase().as_str(), "success" | "ok" | "completed")
    })
}

fn is_retry(event: &AgentEvent) -> bool {
    let name = event.name.to_ascii_lowercase();
    if name.contains("retry") || name.contains("retries") {
        return true;
    }

    event
        .attributes
        .as_object()
        .is_some_and(|attributes| {
            attributes.keys().any(|key| {
                let key = key.to_ascii_lowercase();
                key.contains("retry") || key.contains("attempt")
            })
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::analyze_session;
    use crate::core::model::{AgentEvent, AgentEventKind};

    #[test]
    fn computes_token_efficiency_and_retry_ratio() {
        let mut llm = AgentEvent::new("codex", AgentEventKind::LlmCall, "api_request");
        llm.session_id = Some("s1".into());
        llm.turn_id = Some("t1".into());
        llm.model = Some("gpt-5".into());
        llm.input_tokens = Some(80);
        llm.output_tokens = Some(20);
        llm.cost_usd = Some(0.02);

        let mut decision = AgentEvent::new("codex", AgentEventKind::Decision, "tool_decision");
        decision.session_id = Some("s1".into());
        decision.turn_id = Some("t1".into());

        let mut retry = AgentEvent::new("codex", AgentEventKind::LlmCall, "api_retry");
        retry.session_id = Some("s1".into());
        retry.turn_id = Some("t1".into());
        retry.input_tokens = Some(40);
        retry.output_tokens = Some(10);
        retry.attributes = json!({ "retry_count": 1 });

        let mut tool = AgentEvent::new("codex", AgentEventKind::ToolCall, "tool_result");
        tool.session_id = Some("s1".into());
        tool.turn_id = Some("t1".into());

        let mut outcome = AgentEvent::new("codex", AgentEventKind::Outcome, "finish");
        outcome.session_id = Some("s1".into());
        outcome.turn_id = Some("t1".into());
        outcome.status = Some("success".into());

        let report = analyze_session(&[llm, decision, retry, tool, outcome]).unwrap();

        assert_eq!(report.tokens.total_tokens, 150);
        assert_eq!(report.successful_outcomes, 1);
        assert_eq!(report.retries, 1);
        assert_eq!(report.efficiency.tokens_per_success, Some(150.0));
        assert_eq!(report.efficiency.tokens_per_decision, Some(150.0));
        assert_eq!(report.efficiency.tokens_per_tool_call, Some(150.0));
        assert_eq!(report.efficiency.retry_token_ratio, Some(50.0 / 150.0));
        assert_eq!(report.by_model["gpt-5"].total_tokens, 100);
    }
}
