use std::collections::{BTreeMap, HashMap};

use crate::core::model::AgentEvent;

use super::{
    aggregate::Aggregate,
    types::{SessionAnalytics, TokenTotals, TurnAnalytics},
};

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
