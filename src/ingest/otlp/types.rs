use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct OtlpLogRecord {
    pub event_name: String,
    pub timestamp: DateTime<Utc>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub attributes: Map<String, Value>,
    pub resource_attributes: Map<String, Value>,
    pub body: Value,
}

#[derive(Debug, Clone)]
pub struct OtlpSpanRecord {
    pub name: String,
    pub timestamp: DateTime<Utc>,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub duration_ms: f64,
    pub status: Option<String>,
    pub attributes: Map<String, Value>,
    pub resource_attributes: Map<String, Value>,
}
