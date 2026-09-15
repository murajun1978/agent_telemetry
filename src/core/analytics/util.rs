use serde_json::Value;

use crate::core::model::AgentEvent;

pub(super) fn ratio(value: u64, count: usize) -> Option<f64> {
    (count > 0).then(|| value as f64 / count as f64)
}

pub(super) fn float_ratio(value: f64, count: usize) -> Option<f64> {
    (count > 0).then(|| value / count as f64)
}

pub(super) fn count_ratio(value: usize, count: usize) -> Option<f64> {
    (count > 0).then(|| value as f64 / count as f64)
}

pub(super) fn is_success(event: &AgentEvent) -> bool {
    event.status.as_deref().is_some_and(|status| {
        matches!(
            status.to_ascii_lowercase().as_str(),
            "success" | "ok" | "completed"
        )
    })
}

pub(super) fn is_retry(event: &AgentEvent) -> bool {
    let name = event.name.to_ascii_lowercase();
    if name.contains("retry") || name.contains("retries") {
        return true;
    }

    event.attributes.as_object().is_some_and(|attributes| {
        attributes.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            if key.contains("retry") {
                numeric_value(value).is_some_and(|count| count > 0)
                    || bool_value(value).unwrap_or(false)
            } else if key.contains("attempt") {
                numeric_value(value).is_some_and(|attempt| attempt > 1)
            } else {
                false
            }
        })
    })
}

fn numeric_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn bool_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}
