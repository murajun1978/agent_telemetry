use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    model::AgentEvent,
    store::{EventQuery, TelemetryStore},
};

pub struct GreptimeStore {
    endpoint: String,
    database: String,
    client: Client,
}

impl GreptimeStore {
    pub fn new(endpoint: impl Into<String>, database: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_owned(),
            database: database.into(),
            client: Client::new(),
        }
    }

    async fn sql(&self, sql: &str) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}/v1/sql", self.endpoint))
            .query(&[("db", self.database.as_str())])
            .form(&[("sql", sql)])
            .send()
            .await
            .context("GreptimeDB request failed")?;

        let status = response.status();
        let body: Value = response
            .json()
            .await
            .context("invalid GreptimeDB response")?;
        if !status.is_success() || body.get("error").is_some() {
            return Err(anyhow!("GreptimeDB SQL failed: {body}"));
        }
        Ok(body)
    }
}

#[async_trait]
impl TelemetryStore for GreptimeStore {
    fn name(&self) -> &'static str {
        "greptimedb"
    }

    async fn init(&self) -> Result<()> {
        self.sql(
            "CREATE TABLE IF NOT EXISTS agent_events (\
             id STRING, agent STRING, session_id STRING, kind STRING, name STRING, \
             payload STRING, ts TIMESTAMP TIME INDEX, PRIMARY KEY(agent, id))",
        )
        .await?;
        Ok(())
    }

    async fn append(&self, events: &[AgentEvent]) -> Result<()> {
        for event in events {
            let payload = sql_string(&serde_json::to_string(event)?);
            let id = sql_string(&event.id);
            let agent = sql_string(&event.agent);
            let session = sql_string(event.session_id.as_deref().unwrap_or(""));
            let kind = sql_string(&format!("{:?}", event.kind).to_lowercase());
            let name = sql_string(&event.name);
            let ts = event.timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
            self.sql(&format!(
                "INSERT INTO agent_events (id, agent, session_id, kind, name, payload, ts) \
                 VALUES ({id}, {agent}, {session}, {kind}, {name}, {payload}, '{ts}')"
            ))
            .await?;
        }
        Ok(())
    }

    async fn recent(&self, query: &EventQuery) -> Result<Vec<AgentEvent>> {
        let mut filters = Vec::new();
        if let Some(agent) = &query.agent {
            filters.push(format!("agent = {}", sql_string(agent)));
        }
        if let Some(session) = &query.session_id {
            filters.push(format!("session_id = {}", sql_string(session)));
        }
        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", filters.join(" AND "))
        };
        let limit = query.limit.max(1);
        let body = self
            .sql(&format!(
                "SELECT payload FROM agent_events{where_clause} ORDER BY ts DESC LIMIT {limit}"
            ))
            .await?;

        extract_payload_rows(&body)
    }
}

fn sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn extract_payload_rows(body: &Value) -> Result<Vec<AgentEvent>> {
    let rows = body
        .pointer("/output/0/records/rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    rows.into_iter()
        .filter_map(|row| row.as_array().and_then(|cells| cells.first()).cloned())
        .filter_map(|cell| cell.as_str().map(ToOwned::to_owned))
        .map(|json| serde_json::from_str(&json).context("invalid event payload from GreptimeDB"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sql_string;

    #[test]
    fn escapes_sql_strings() {
        assert_eq!(sql_string("agent's"), "'agent''s'");
    }
}
