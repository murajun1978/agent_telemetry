use std::path::Path;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::core::model::AgentEvent;

pub async fn read_events(path: impl AsRef<Path>) -> Result<Vec<AgentEvent>> {
    let file = tokio::fs::File::open(path.as_ref())
        .await
        .with_context(|| format!("failed to open {}", path.as_ref().display()))?;
    let mut lines = BufReader::new(file).lines();
    let mut events = Vec::new();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let event: AgentEvent = serde_json::from_str(&line).context("invalid AgentEvent JSONL")?;
        events.push(event);
    }

    Ok(events)
}
