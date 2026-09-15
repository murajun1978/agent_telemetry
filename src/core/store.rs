use anyhow::Result;
use async_trait::async_trait;

use super::model::AgentEvent;

#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub limit: usize,
}

#[async_trait]
pub trait TelemetryStore: Send + Sync {
    fn name(&self) -> &'static str;
    async fn init(&self) -> Result<()>;
    async fn append(&self, events: &[AgentEvent]) -> Result<()>;
    async fn recent(&self, query: &EventQuery) -> Result<Vec<AgentEvent>>;
}
