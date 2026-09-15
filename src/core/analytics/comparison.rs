use std::collections::{BTreeMap, BTreeSet};

use crate::core::model::AgentEvent;

use super::{
    aggregate::Aggregate,
    types::{AgentComparison, ComparisonReport, ModelComparison, SessionComparison, TokenTotals},
    util::{count_ratio, float_ratio, ratio},
};

#[derive(Debug, Clone, Default)]
struct AgentRollup {
    aggregate: Aggregate,
    session_tokens: TokenTotals,
    sessions: usize,
    outcome_observed_sessions: usize,
    successful_sessions: usize,
}

#[derive(Debug, Clone, Default)]
struct ModelRollup {
    agents: BTreeSet<String>,
    events: usize,
    tokens: TokenTotals,
}

pub fn compare_events(events: &[AgentEvent]) -> ComparisonReport {
    let mut agent_rollups: BTreeMap<String, AgentRollup> = BTreeMap::new();
    let mut model_rollups: BTreeMap<String, ModelRollup> = BTreeMap::new();
    let mut session_rollups: BTreeMap<(String, String), Aggregate> = BTreeMap::new();
    let mut unscoped_events = 0;

    for event in events {
        agent_rollups
            .entry(event.agent.clone())
            .or_default()
            .aggregate
            .add_event_without_flow(event);

        if let Some(session_id) = &event.session_id {
            session_rollups
                .entry((event.agent.clone(), session_id.clone()))
                .or_default()
                .add_event_without_flow(event);
        } else {
            unscoped_events += 1;
        }

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
            let successful = if aggregate.outcomes == 0 {
                None
            } else {
                Some(aggregate.successful_outcomes > 0)
            };
            let agent_rollup = agent_rollups.entry(agent.clone()).or_default();
            agent_rollup.sessions += 1;
            agent_rollup.session_tokens.add_totals(&aggregate.tokens);
            if let Some(successful) = successful {
                agent_rollup.outcome_observed_sessions += 1;
                if successful {
                    agent_rollup.successful_sessions += 1;
                }
            }

            SessionComparison {
                agent,
                session_id,
                successful,
                tokens: aggregate.tokens.clone(),
                decisions: aggregate.decisions,
                tool_calls: aggregate.tool_calls,
                actions: aggregate.actions,
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
            outcome_observed_sessions: rollup.outcome_observed_sessions,
            successful_sessions: rollup.successful_sessions,
            session_success_rate: count_ratio(
                rollup.successful_sessions,
                rollup.outcome_observed_sessions,
            ),
            average_tokens_per_session: ratio(rollup.session_tokens.total_tokens, rollup.sessions),
            average_cost_per_session: float_ratio(rollup.session_tokens.cost_usd, rollup.sessions),
            tokens_per_successful_session: ratio(
                rollup.session_tokens.total_tokens,
                rollup.successful_sessions,
            ),
            cost_per_successful_session: float_ratio(
                rollup.session_tokens.cost_usd,
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
        unscoped_events,
        sessions: session_details.len(),
        agents,
        models,
        session_details,
    }
}
