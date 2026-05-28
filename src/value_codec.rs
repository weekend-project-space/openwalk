use scheme4r::{runtime::DictKey, SchemeString, Value};

use crate::{browser::BrowserValue, tool_metadata::ToolMetadata};

pub(crate) fn browser_value_to_scheme(value: BrowserValue) -> Value {
    match value {
        BrowserValue::Unit => Value::Unspecified,
        BrowserValue::Boolean(value) => Value::Boolean(value),
        BrowserValue::Number(value) => Value::Number(value),
        BrowserValue::String(value) => Value::String(SchemeString::new(value)),
        BrowserValue::Array(values) => {
            Value::list(values.into_iter().map(browser_value_to_scheme).collect())
        }
        BrowserValue::Object(values) => Value::list(
            values
                .into_iter()
                .map(|(key, value)| Value::pair(Value::string(key), browser_value_to_scheme(value)))
                .collect(),
        ),
    }
}

pub(crate) fn tool_metadata_to_scheme(metadata: &ToolMetadata) -> Value {
    let json = serde_json::to_value(metadata)
        .expect("ToolMetadata should always serialize into JSON successfully");
    json_to_scheme_value(json)
}

pub(crate) fn json_to_scheme_value(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Unspecified,
        serde_json::Value::Bool(value) => Value::Boolean(value),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(Value::Number)
            .unwrap_or_else(|| Value::string(value.to_string())),
        serde_json::Value::String(value) => Value::string(value),
        serde_json::Value::Array(values) => {
            Value::list(values.into_iter().map(json_to_scheme_value).collect())
        }
        serde_json::Value::Object(values) => Value::list(
            values
                .into_iter()
                .map(|(key, value)| Value::pair(Value::string(key), json_to_scheme_value(value)))
                .collect(),
        ),
    }
}

pub(crate) fn scheme_value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Boolean(value) => serde_json::Value::Bool(*value),
        Value::Number(value) => serde_json::Value::Number((*value).into()),
        Value::Character(value) => serde_json::Value::String(value.to_string()),
        Value::String(value) => serde_json::Value::String(value.to_plain_string()),
        Value::Symbol(value) => serde_json::Value::String(value.clone()),
        Value::Vector(values) => serde_json::Value::Array(
            values
                .borrow()
                .iter()
                .map(scheme_value_to_json)
                .collect::<Vec<_>>(),
        ),
        Value::ByteVector(values) => serde_json::Value::Array(
            values
                .borrow()
                .iter()
                .map(|value| serde_json::Value::Number(i64::from(*value).into()))
                .collect::<Vec<_>>(),
        ),
        Value::Dict(values) => {
            let mut map = serde_json::Map::new();
            for (key, value) in values.borrow().iter() {
                map.insert(dict_key_to_string(key), scheme_value_to_json(value));
            }
            serde_json::Value::Object(map)
        }
        Value::Record(record) => {
            let record = record.borrow();
            let record_type = record.record_type();
            let mut fields = serde_json::Map::new();
            for index in 0..record_type.field_count() {
                let field_name = record_type
                    .field_name(index)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("field-{index}"));
                if let Some(value) = record.field(index) {
                    fields.insert(field_name, scheme_value_to_json(value));
                }
            }

            let mut map = serde_json::Map::new();
            map.insert(
                "$record".to_string(),
                serde_json::Value::String(record_type.name().to_string()),
            );
            map.insert("fields".to_string(), serde_json::Value::Object(fields));
            serde_json::Value::Object(map)
        }
        Value::Pair(_) | Value::EmptyList => list_or_pair_to_json(value),
        Value::Multiple(values) => {
            serde_json::Value::Array(values.iter().map(scheme_value_to_json).collect())
        }
        Value::Unspecified | Value::EofObject => serde_json::Value::Null,
        Value::Port(_)
        | Value::ErrorObject(_)
        | Value::Parameter(_)
        | Value::Continuation(_)
        | Value::Procedure(_) => serde_json::Value::String(value.to_string()),
    }
}

fn dict_key_to_string(key: &DictKey) -> String {
    match key {
        DictKey::Boolean(value) => value.to_string(),
        DictKey::Number(value) => value.to_string(),
        DictKey::Character(value) => value.to_string(),
        DictKey::String(value) => value.clone(),
        DictKey::Symbol(value) => value.clone(),
        DictKey::EmptyList => "()".to_string(),
    }
}

fn list_or_pair_to_json(value: &Value) -> serde_json::Value {
    if let Some(items) = value.to_proper_list_vec() {
        if let Some(object) = maybe_alist_to_json_object(items.as_slice()) {
            return serde_json::Value::Object(object);
        }
        return serde_json::Value::Array(items.iter().map(scheme_value_to_json).collect());
    }

    if let Value::Pair(pair) = value {
        let pair = pair.borrow();
        let mut map = serde_json::Map::new();
        map.insert("car".to_string(), scheme_value_to_json(&pair.car));
        map.insert("cdr".to_string(), scheme_value_to_json(&pair.cdr));
        return serde_json::Value::Object(map);
    }

    serde_json::Value::String(value.to_string())
}

fn maybe_alist_to_json_object(
    items: &[Value],
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut map = serde_json::Map::new();
    for item in items {
        let Value::Pair(entry) = item else {
            return None;
        };
        let entry = entry.borrow();
        let key = match &entry.car {
            Value::String(value) => value.to_plain_string(),
            Value::Symbol(value) => value.clone(),
            _ => return None,
        };
        map.insert(key, scheme_value_to_json(&entry.cdr));
    }
    Some(map)
}
