use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub session_id: Option<String>,
    pub successful: bool,
    pub tokens: TokenTotals,
    pub decisions: usize,
    pub tool_calls: usize,
    pub outcomes: usize,
    pub errors: usize,
    pub retries: usize,
    pub efficiency: TokenEfficiency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentComparison {
    pub agent: String,
    pub sessions: usize,
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
    pub sessions: usize,
    pub agents: Vec<AgentComparison>,
    pub models: Vec<ModelComparison>,
    pub session_details: Vec<SessionComparison>,
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
    flow: Vec<FlowEvent>,
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

        if matches!(
            event.kind,
            AgentEventKind::Decision
                | AgentEventKind::Action
                | AgentEventKind::Outcome
                | AgentEventKind::ToolCall
                | AgentEventKind::Error
        ) {
            self.flow.push(FlowEvent {
                id: event.id.clone(),
                timestamp: event.timestamp,
                kind: event.kind.clone(),
                name: event.name.clone(),
                status: event.status.clone(),
                tool_name: event.tool_name.clone(),
                model: event.model.clone(),
                tokens: event_tokens,
                cost_usd: usage.cost_usd.unwrap_or(0.0),
            });
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

#[derive(Debug, Clone, Default)]
struct AgentRollup {
    aggregate: Aggregate,
    sessions: usize,
    successful_sessions: usize,
}

#[derive(Debug, Clone, Default)]
struct ModelRollup {
    agents: BTreeSet<String>,
    events: usize,
    tokens: TokenTotals,
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
        .map(|(turn_id, mut values)| {
            values.flow.sort_by_key(|event| event.timestamp);
            TurnAnalytics {
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
                flow: values.flow,
            }
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

pub fn compare_events(events: &[AgentEvent]) -> ComparisonReport {
    let mut agent_rollups: BTreeMap<String, AgentRollup> = BTreeMap::new();
    let mut model_rollups: BTreeMap<String, ModelRollup> = BTreeMap::new();
    let mut session_rollups: BTreeMap<(String, Option<String>), Aggregate> = BTreeMap::new();

    for event in events {
        agent_rollups
            .entry(event.agent.clone())
            .or_default()
            .aggregate
            .add_event(event);

        session_rollups
            .entry((event.agent.clone(), event.session_id.clone()))
            .or_default()
            .add_event(event);

        if let Some(model) = &event.model {
            let model_rollup = model_rollups.entry(model.clone()).or_default();
            model_rollup.agents.insert(event.agent.clone());
            model_rollup.events += 1;
            model_rollup.tokens.add_event(event);
        }
    }

    let session_details = session_rollups
        .into_iter()
        .map(|((agent, session_id), aggregate)| {
            let successful = aggregate.successful_outcomes > 0;
            let agent_rollup = agent_rollups.entry(agent.clone()).or_default();
            agent_rollup.sessions += 1;
            if successful {
                agent_rollup.successful_sessions += 1;
            }

            SessionComparison {
                agent,
                session_id,
                successful,
                tokens: aggregate.tokens.clone(),
                decisions: aggregate.decisions,
                tool_calls: aggregate.tool_calls,
                outcomes: aggregate.outcomes,
                errors: aggregate.errors,
                retries: aggregate.retries,
                efficiency: aggregate.efficiency(),
            }
        })
        .collect::<Vec<_>>();

    let agents = agent_rollups
        .into_iter()
        .map(|(agent, rollup)| AgentComparison {
            agent,
            sessions: rollup.sessions,
            successful_sessions: rollup.successful_sessions,
            session_success_rate: count_ratio(rollup.successful_sessions, rollup.sessions),
            average_tokens_per_session: ratio(
                rollup.aggregate.tokens.total_tokens,
                rollup.sessions,
            ),
            average_cost_per_session: float_ratio(rollup.aggregate.tokens.cost_usd, rollup.sessions),
            tokens_per_successful_session: ratio(
                rollup.aggregate.tokens.total_tokens,
                rollup.successful_sessions,
            ),
            cost_per_successful_session: float_ratio(
                rollup.aggregate.tokens.cost_usd,
                rollup.successful_sessions,
            ),
            tokens: rollup.aggregate.tokens.clone(),
            decisions: rollup.aggregate.decisions,
            tool_calls: rollup.aggregate.tool_calls,
            actions: rollup.aggregate.actions,
            outcomes: rollup.aggregate.outcomes,
            errors: rollup.aggregate.errors,
            retries: rollup.aggregate.retries,
            efficiency: rollup.aggregate.efficiency(),
        })
        .collect();

    let models = model_rollups
        .into_iter()
        .map(|(model, rollup)| ModelComparison {
            model,
            agents: rollup.agents.into_iter().collect(),
            events: rollup.events,
            tokens: rollup.tokens,
        })
        .collect();

    ComparisonReport {
        events: events.len(),
        sessions: session_details.len(),
        agents,
        models,
        session_details,
    }
}

fn ratio(value: u64, count: usize) -> Option<f64> {
    (count > 0).then(|| value as f64 / count as f64)
}

fn float_ratio(value: f64, count: usize) -> Option<f64> {
    (count > 0).then(|| value / count as f64)
}

fn count_ratio(value: usize, count: usize) -> Option<f64> {
    (count > 0).then(|| value as f64 / count as f64)
}

fn is_success(event: &AgentEvent) -> bool {
    event.status.as_deref().is_some_and(|status| {
        matches!(
            status.to_ascii_lowercase().as_str(),
            "success" | "ok" | "completed"
        )
    })
}

fn is_retry(event: &AgentEvent) -> bool {
    let name = event.name.to_ascii_lowercase();
    if name.contains("retry") || name.contains("retries") {
        return true;
    }

    event.attributes.as_object().is_some_and(|attributes| {
        attributes.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            if key.contains("retry") {
                numeric_value(value).is_some_and(|count| count > 0)
                    || bool_value(value).unwrap_or(false)
            } else if key.contains("attempt") {
                numeric_value(value).is_some_and(|attempt| attempt > 1)
            } else {
                false
            }
        })
    })
}

fn numeric_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn bool_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use serde_json::json;

    use super::{analyze_session, compare_events};
    use crate::core::model::{AgentEvent, AgentEventKind};

    #[test]
    fn computes_token_efficiency_and_correlated_flow() {
        let base = Utc::now();
        let mut llm = AgentEvent::new("codex", AgentEventKind::LlmCall, "api_request");
        llm.timestamp = base;
        llm.session_id = Some("s1".into());
        llm.turn_id = Some("t1".into());
        llm.model = Some("gpt-5".into());
        llm.input_tokens = Some(80);
        llm.output_tokens = Some(20);
        llm.cost_usd = Some(0.02);

        let mut decision = AgentEvent::new("codex", AgentEventKind::Decision, "tool_decision");
        decision.timestamp = base + Duration::milliseconds(1);
        decision.session_id = Some("s1".into());
        decision.turn_id = Some("t1".into());

        let mut retry = AgentEvent::new("codex", AgentEventKind::LlmCall, "api_retry");
        retry.timestamp = base + Duration::milliseconds(2);
        retry.session_id = Some("s1".into());
        retry.turn_id = Some("t1".into());
        retry.input_tokens = Some(40);
        retry.output_tokens = Some(10);
        retry.attributes = json!({ "retry_count": 1 });

        let mut tool = AgentEvent::new("codex", AgentEventKind::ToolCall, "tool_result");
        tool.timestamp = base + Duration::milliseconds(3);
        tool.session_id = Some("s1".into());
        tool.turn_id = Some("t1".into());

        let mut outcome = AgentEvent::new("codex", AgentEventKind::Outcome, "finish");
        outcome.timestamp = base + Duration::milliseconds(4);
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
        assert_eq!(report.turns[0].flow.len(), 3);
        assert_eq!(report.turns[0].flow[0].kind, AgentEventKind::Decision);
        assert_eq!(report.turns[0].flow[1].kind, AgentEventKind::ToolCall);
        assert_eq!(report.turns[0].flow[2].kind, AgentEventKind::Outcome);
    }

    #[test]
    fn first_attempt_and_zero_retry_count_are_not_retries() {
        let mut first_attempt = AgentEvent::new("agent", AgentEventKind::LlmCall, "api_request");
        first_attempt.input_tokens = Some(10);
        first_attempt.attributes = json!({ "attempt": 1, "retry_count": 0 });

        let mut second_attempt = AgentEvent::new("agent", AgentEventKind::LlmCall, "api_request");
        second_attempt.input_tokens = Some(20);
        second_attempt.attributes = json!({ "attempt": 2 });

        let report = analyze_session(&[first_attempt, second_attempt]).unwrap();

        assert_eq!(report.retries, 1);
        assert_eq!(report.efficiency.retry_token_ratio, Some(20.0 / 30.0));
    }

    #[test]
    fn compares_agents_sessions_and_models_without_ranking() {
        let mut codex_llm = AgentEvent::new("codex", AgentEventKind::LlmCall, "api_request");
        codex_llm.session_id = Some("codex-1".into());
        codex_llm.model = Some("gpt-5".into());
        codex_llm.input_tokens = Some(80);
        codex_llm.output_tokens = Some(20);
        codex_llm.cost_usd = Some(0.02);

        let mut codex_outcome = AgentEvent::new("codex", AgentEventKind::Outcome, "finish");
        codex_outcome.session_id = Some("codex-1".into());
        codex_outcome.status = Some("success".into());

        let mut gemini_llm =
            AgentEvent::new("gemini-cli", AgentEventKind::LlmCall, "generate_content");
        gemini_llm.session_id = Some("gemini-1".into());
        gemini_llm.model = Some("gemini-2.5-pro".into());
        gemini_llm.input_tokens = Some(120);
        gemini_llm.output_tokens = Some(30);
        gemini_llm.cost_usd = Some(0.03);

        let mut gemini_outcome =
            AgentEvent::new("gemini-cli", AgentEventKind::Outcome, "finish");
        gemini_outcome.session_id = Some("gemini-1".into());
        gemini_outcome.status = Some("error".into());

        let report = compare_events(&[codex_llm, codex_outcome, gemini_llm, gemini_outcome]);

        assert_eq!(report.events, 4);
        assert_eq!(report.sessions, 2);
        assert_eq!(report.agents.len(), 2);
        assert_eq!(report.models.len(), 2);

        let codex = report
            .agents
            .iter()
            .find(|row| row.agent == "codex")
            .unwrap();
        assert_eq!(codex.sessions, 1);
        assert_eq!(codex.successful_sessions, 1);
        assert_eq!(codex.session_success_rate, Some(1.0));
        assert_eq!(codex.tokens.total_tokens, 100);
        assert_eq!(codex.tokens_per_successful_session, Some(100.0));

        let gemini = report
            .agents
            .iter()
            .find(|row| row.agent == "gemini-cli")
            .unwrap();
        assert_eq!(gemini.sessions, 1);
        assert_eq!(gemini.successful_sessions, 0);
        assert_eq!(gemini.session_success_rate, Some(0.0));
        assert_eq!(gemini.tokens.total_tokens, 150);
        assert_eq!(gemini.tokens_per_successful_session, None);
    }
}
