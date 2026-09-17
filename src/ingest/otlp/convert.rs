use chrono::{DateTime, Utc};
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value};
use serde_json::{Map, Number, Value};

pub(super) fn attributes_to_json(attributes: &[KeyValue]) -> Map<String, Value> {
    attributes
        .iter()
        .filter_map(|attribute| {
            attribute
                .value
                .as_ref()
                .map(|value| (attribute.key.clone(), any_value_to_json(value)))
        })
        .collect()
}

pub(super) fn any_value_to_json(value: &AnyValue) -> Value {
    match value.value.as_ref() {
        Some(any_value::Value::StringValue(value)) => Value::String(value.clone()),
        Some(any_value::Value::StringValueStrindex(value)) => {
            Value::String(format!("#strindex:{value}"))
        }
        Some(any_value::Value::BoolValue(value)) => Value::Bool(*value),
        Some(any_value::Value::IntValue(value)) => Value::Number((*value).into()),
        Some(any_value::Value::DoubleValue(value)) => Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Some(any_value::Value::ArrayValue(value)) => {
            Value::Array(value.values.iter().map(any_value_to_json).collect())
        }
        Some(any_value::Value::KvlistValue(value)) => Value::Object(
            value
                .values
                .iter()
                .filter_map(|entry| {
                    entry
                        .value
                        .as_ref()
                        .map(|value| (entry.key.clone(), any_value_to_json(value)))
                })
                .collect(),
        ),
        Some(any_value::Value::BytesValue(value)) => Value::String(hex::encode(value)),
        None => Value::Null,
    }
}

pub(super) fn unix_nanos(nanos: u64) -> DateTime<Utc> {
    if nanos == 0 {
        return Utc::now();
    }
    let seconds = (nanos / 1_000_000_000) as i64;
    let subsec_nanos = (nanos % 1_000_000_000) as u32;
    DateTime::<Utc>::from_timestamp(seconds, subsec_nanos).unwrap_or_else(Utc::now)
}

pub(super) fn id_or_none(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| hex::encode(bytes))
}
