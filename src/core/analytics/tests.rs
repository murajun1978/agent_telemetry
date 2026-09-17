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
fn compares_multiple_sessions_shared_models_and_unknowns() {
    let mut codex_one = AgentEvent::new("codex", AgentEventKind::LlmCall, "api_request");
    codex_one.session_id = Some("codex-1".into());
    codex_one.model = Some("shared-model".into());
    codex_one.input_tokens = Some(80);
    codex_one.output_tokens = Some(20);
    codex_one.cost_usd = Some(0.02);

    let mut codex_one_action = AgentEvent::new("codex", AgentEventKind::Action, "apply_patch");
    codex_one_action.session_id = Some("codex-1".into());

    let mut codex_one_outcome = AgentEvent::new("codex", AgentEventKind::Outcome, "finish");
    codex_one_outcome.session_id = Some("codex-1".into());
    codex_one_outcome.status = Some("success".into());

    let mut codex_two = AgentEvent::new("codex", AgentEventKind::LlmCall, "api_request");
    codex_two.session_id = Some("codex-2".into());
    codex_two.model = Some("gpt-5".into());
    codex_two.input_tokens = Some(40);
    codex_two.output_tokens = Some(10);
    codex_two.cost_usd = Some(0.01);

    let mut gemini = AgentEvent::new("gemini-cli", AgentEventKind::LlmCall, "generate_content");
    gemini.session_id = Some("gemini-1".into());
    gemini.model = Some("shared-model".into());
    gemini.input_tokens = Some(120);
    gemini.output_tokens = Some(30);
    gemini.cost_usd = Some(0.03);

    let mut gemini_outcome = AgentEvent::new("gemini-cli", AgentEventKind::Outcome, "finish");
    gemini_outcome.session_id = Some("gemini-1".into());
    gemini_outcome.status = Some("error".into());

    let mut unscoped = AgentEvent::new("generic-otel", AgentEventKind::LlmCall, "chat");
    unscoped.input_tokens = Some(999);

    let report = compare_events(&[
        codex_one,
        codex_one_action,
        codex_one_outcome,
        codex_two,
        gemini,
        gemini_outcome,
        unscoped,
    ]);

    assert_eq!(report.events, 7);
    assert_eq!(report.unscoped_events, 1);
    assert_eq!(report.sessions, 3);
    assert_eq!(report.session_details.len(), 3);

    let codex = report
        .agents
        .iter()
        .find(|row| row.agent == "codex")
        .unwrap();
    assert_eq!(codex.sessions, 2);
    assert_eq!(codex.outcome_observed_sessions, 1);
    assert_eq!(codex.successful_sessions, 1);
    assert_eq!(codex.session_success_rate, Some(1.0));
    assert_eq!(codex.average_tokens_per_session, Some(75.0));
    assert_eq!(codex.tokens_per_successful_session, Some(150.0));

    let codex_two = report
        .session_details
        .iter()
        .find(|row| row.session_id == "codex-2")
        .unwrap();
    assert_eq!(codex_two.successful, None);

    let codex_one = report
        .session_details
        .iter()
        .find(|row| row.session_id == "codex-1")
        .unwrap();
    assert_eq!(codex_one.successful, Some(true));
    assert_eq!(codex_one.actions, 1);

    let gemini = report
        .session_details
        .iter()
        .find(|row| row.session_id == "gemini-1")
        .unwrap();
    assert_eq!(gemini.successful, Some(false));

    let shared = report
        .models
        .iter()
        .find(|row| row.model == "shared-model")
        .unwrap();
    assert_eq!(shared.agents, vec!["codex", "gemini-cli"]);
    assert_eq!(shared.events, 2);
    assert_eq!(shared.tokens.total_tokens, 250);

    let generic = report
        .agents
        .iter()
        .find(|row| row.agent == "generic-otel")
        .unwrap();
    assert_eq!(generic.sessions, 0);
    assert_eq!(generic.session_success_rate, None);
    assert_eq!(generic.average_tokens_per_session, None);
    assert_eq!(generic.tokens.total_tokens, 999);
}
