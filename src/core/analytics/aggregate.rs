use crate::core::model::{AgentEvent, AgentEventKind};

use super::{
    types::{FlowEvent, TokenEfficiency, TokenTotals},
    util::{float_ratio, is_retry, is_success, ratio},
};

#[derive(Debug, Clone, Default)]
pub(super) struct Aggregate {
    pub tokens: TokenTotals,
    pub decisions: usize,
    pub tool_calls: usize,
    pub actions: usize,
    pub outcomes: usize,
    pub successful_outcomes: usize,
    pub errors: usize,
    pub retries: usize,
    retry_tokens: u64,
    pub flow: Vec<FlowEvent>,
}

impl Aggregate {
    pub fn add_event(&mut self, event: &AgentEvent) {
        self.add_event_internal(event, true);
    }

    pub fn add_event_without_flow(&mut self, event: &AgentEvent) {
        self.add_event_internal(event, false);
    }

    fn add_event_internal(&mut self, event: &AgentEvent, capture_flow: bool) {
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

        if capture_flow
            && matches!(
                event.kind,
                AgentEventKind::Decision
                    | AgentEventKind::Action
                    | AgentEventKind::Outcome
                    | AgentEventKind::ToolCall
                    | AgentEventKind::Error
            )
        {
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

    pub fn efficiency(&self) -> TokenEfficiency {
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
