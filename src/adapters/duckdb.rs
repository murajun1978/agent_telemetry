use std::{path::Path, sync::Mutex};

use anyhow::{Context, Result};
use async_trait::async_trait;
use duckdb::{Connection, params};

use crate::core::{
    model::AgentEvent,
    store::{EventQuery, TelemetryStore},
};

pub struct DuckDbStore {
    connection: Mutex<Connection>,
}

impl DuckDbStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path).context("failed to open DuckDB")?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}

#[async_trait]
impl TelemetryStore for DuckDbStore {
    fn name(&self) -> &'static str {
        "duckdb"
    }

    async fn init(&self) -> Result<()> {
        self.connection.lock().unwrap().execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_events (\
             id VARCHAR PRIMARY KEY, agent VARCHAR NOT NULL, session_id VARCHAR, kind VARCHAR NOT NULL, \
             name VARCHAR NOT NULL, payload JSON NOT NULL, ts TIMESTAMPTZ NOT NULL);",
        )?;
        Ok(())
    }

    async fn append(&self, events: &[AgentEvent]) -> Result<()> {
        let mut conn = self.connection.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO agent_events \
                 (id, agent, session_id, kind, name, payload, ts) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )?;
            for event in events {
                stmt.execute(params![
                    event.id,
                    event.agent,
                    event.session_id,
                    format!("{:?}", event.kind).to_lowercase(),
                    event.name,
                    serde_json::to_string(event)?,
                    event.timestamp.to_rfc3339(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    async fn recent(&self, query: &EventQuery) -> Result<Vec<AgentEvent>> {
        let conn = self.connection.lock().unwrap();
        let limit = query.limit.max(1) as i64;
        let (sql, args): (&str, Vec<String>) = match (&query.agent, &query.session_id) {
            (Some(agent), Some(session)) => (
                "SELECT payload FROM agent_events WHERE agent = ? AND session_id = ? ORDER BY ts DESC LIMIT ?",
                vec![agent.clone(), session.clone()],
            ),
            (Some(agent), None) => (
                "SELECT payload FROM agent_events WHERE agent = ? ORDER BY ts DESC LIMIT ?",
                vec![agent.clone()],
            ),
            (None, Some(session)) => (
                "SELECT payload FROM agent_events WHERE session_id = ? ORDER BY ts DESC LIMIT ?",
                vec![session.clone()],
            ),
            (None, None) => (
                "SELECT payload FROM agent_events ORDER BY ts DESC LIMIT ?",
                vec![],
            ),
        };

        let mut stmt = conn.prepare(sql)?;
        let mut rows = match args.as_slice() {
            [a, b] => stmt.query(params![a, b, limit])?,
            [a] => stmt.query(params![a, limit])?,
            [] => stmt.query(params![limit])?,
            _ => unreachable!(),
        };
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            events.push(serde_json::from_str(&payload)?);
        }
        Ok(events)
    }
}
