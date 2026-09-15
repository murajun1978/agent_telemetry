mod aggregate;
mod comparison;
mod session;
mod types;
mod util;

pub use comparison::compare_events;
pub use session::analyze_session;
pub use types::{
    AgentComparison, ComparisonReport, FlowEvent, ModelComparison, SessionAnalytics,
    SessionComparison, TokenEfficiency, TokenTotals, TurnAnalytics,
};

#[cfg(test)]
mod tests;
