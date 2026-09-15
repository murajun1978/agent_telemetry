mod claude;
mod codex;
mod generic;

use std::sync::Arc;

use crate::{
    core::model::AgentEvent,
    ingest::otlp::{OtlpLogRecord, OtlpSpanRecord},
};

pub use claude::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use generic::GenericOtelAdapter;

pub trait SemanticAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn normalize_log(&self, record: &OtlpLogRecord) -> Option<AgentEvent>;
    fn normalize_span(&self, record: &OtlpSpanRecord) -> Option<AgentEvent>;
}

#[derive(Clone)]
pub struct AdapterRegistry {
    adapters: Vec<Arc<dyn SemanticAdapter>>,
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self {
            adapters: vec![
                Arc::new(ClaudeCodeAdapter),
                Arc::new(CodexAdapter),
                Arc::new(GenericOtelAdapter),
            ],
        }
    }
}

impl AdapterRegistry {
    pub fn normalize_log(&self, record: &OtlpLogRecord) -> Option<AgentEvent> {
        self.adapters
            .iter()
            .find_map(|adapter| adapter.normalize_log(record))
    }

    pub fn normalize_span(&self, record: &OtlpSpanRecord) -> Option<AgentEvent> {
        self.adapters
            .iter()
            .find_map(|adapter| adapter.normalize_span(record))
    }
}
