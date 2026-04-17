use super::{
    types::{BrowserValue, Locator, LocatorKind},
    *,
};

const DEFAULT_BROWSER_REQUEST_TIMEOUT_SECS: u64 = 30;
const DEFAULT_SESSION_CONNECT_TIMEOUT_SECS: u64 = 30;

pub(super) fn js_locator_function(locator: &Locator, body: String) -> String {
    match locator {
        Locator::Css(selector) => format!(
            r#"() => {{
                const el = document.querySelector({selector:?});
                if (!el) {{
                    throw new Error("selector not found");
                }}
                {body}
            }}"#
        ),
        Locator::XPath(xpath) => format!(
            r#"() => {{
                const result = document.evaluate(
                    {xpath:?},
                    document,
                    null,
                    XPathResult.FIRST_ORDERED_NODE_TYPE,
                    null
                );
                const el = result.singleNodeValue;
                if (!el) {{
                    throw new Error("xpath not found");
                }}
                {body}
            }}"#
        ),
    }
}

pub(super) fn flat_attributes_to_map(items: Vec<String>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in items.chunks(2) {
        if let [name, value] = pair {
            map.insert(name.clone(), value.clone());
        }
    }
    map
}

pub(super) fn locator_name(kind: LocatorKind) -> &'static str {
    match kind {
        LocatorKind::Css => "selector",
        LocatorKind::XPath => "xpath",
    }
}

pub(super) fn env_flag_is_truthy(name: &str) -> bool {
    env::var(name)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

pub(super) fn env_flag_is_false(name: &str) -> bool {
    env::var(name)
        .map(|value| matches!(value.as_str(), "0" | "false" | "FALSE" | "no" | "NO"))
        .unwrap_or(false)
}

pub(super) fn browser_request_timeout() -> Duration {
    env_duration_secs(
        "OPENWALK_CDP_TIMEOUT_SECS",
        DEFAULT_BROWSER_REQUEST_TIMEOUT_SECS,
    )
}

pub(super) fn session_connect_timeout() -> Duration {
    env_duration_secs(
        "OPENWALK_SESSION_CONNECT_TIMEOUT_SECS",
        DEFAULT_SESSION_CONNECT_TIMEOUT_SECS,
    )
}

fn env_duration_secs(name: &str, default_secs: u64) -> Duration {
    duration_secs_from_env_value(env::var(name).ok().as_deref(), default_secs)
}

fn duration_secs_from_env_value(raw: Option<&str>, default_secs: u64) -> Duration {
    raw.and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(default_secs))
}

pub(super) fn json_to_browser_value(value: serde_json::Value) -> BrowserValue {
    match value {
        serde_json::Value::Null => BrowserValue::Unit,
        serde_json::Value::Bool(value) => BrowserValue::Boolean(value),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(BrowserValue::Number)
            .unwrap_or_else(|| BrowserValue::String(value.to_string())),
        serde_json::Value::String(value) => BrowserValue::String(value),
        serde_json::Value::Array(values) => {
            BrowserValue::Array(values.into_iter().map(json_to_browser_value).collect())
        }
        serde_json::Value::Object(values) => BrowserValue::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, json_to_browser_value(value)))
                .collect(),
        ),
    }
}

pub(super) fn serialize_to_browser_value<T>(
    value: &T,
    context: &'static str,
) -> Result<BrowserValue>
where
    T: serde::Serialize,
{
    let json = serde_json::to_value(value).context(context)?;
    Ok(json_to_browser_value(json))
}

pub fn parse_mouse_button(input: &str) -> Result<MouseButton> {
    MouseButton::from_str(input).map_err(|_| anyhow!("unsupported mouse button `{input}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn duration_secs_from_env_value_accepts_positive_integers() {
        assert_eq!(
            duration_secs_from_env_value(Some("45"), 30),
            Duration::from_secs(45)
        );
    }

    #[test]
    fn duration_secs_from_env_value_falls_back_for_invalid_values() {
        assert_eq!(
            duration_secs_from_env_value(Some("0"), 30),
            Duration::from_secs(30)
        );
        assert_eq!(
            duration_secs_from_env_value(Some("abc"), 30),
            Duration::from_secs(30)
        );
        assert_eq!(
            duration_secs_from_env_value(None, 30),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn parse_mouse_button_supports_common_values() {
        assert!(matches!(
            parse_mouse_button("left").unwrap(),
            MouseButton::Left
        ));
        assert!(matches!(
            parse_mouse_button("right").unwrap(),
            MouseButton::Right
        ));
        assert!(parse_mouse_button("weird").is_err());
    }

    #[test]
    fn json_to_browser_value_maps_primitives() {
        assert!(matches!(
            json_to_browser_value(serde_json::Value::Null),
            BrowserValue::Unit
        ));
        assert!(matches!(
            json_to_browser_value(serde_json::Value::Bool(true)),
            BrowserValue::Boolean(true)
        ));
        assert!(matches!(
            json_to_browser_value(json!(42)),
            BrowserValue::Number(42)
        ));
        assert!(matches!(
            json_to_browser_value(json!("ok")),
            BrowserValue::String(value) if value == "ok"
        ));
    }

    #[test]
    fn json_to_browser_value_serializes_non_integer_numbers() {
        assert!(matches!(
            json_to_browser_value(json!(3.14)),
            BrowserValue::String(value) if value == "3.14"
        ));
    }

    #[test]
    fn json_to_browser_value_maps_arrays_and_objects_recursively() {
        let array = json_to_browser_value(json!([1, true, "ok"]));
        assert!(matches!(
            array,
            BrowserValue::Array(values)
            if matches!(values.as_slice(), [
                BrowserValue::Number(1),
                BrowserValue::Boolean(true),
                BrowserValue::String(text)
            ] if text == "ok")
        ));

        let object = json_to_browser_value(json!({"a": 1, "b": [2, 3]}));
        let BrowserValue::Object(values) = object else {
            panic!("expected object browser value");
        };
        assert!(matches!(values.get("a"), Some(BrowserValue::Number(1))));
        assert!(matches!(
            values.get("b"),
            Some(BrowserValue::Array(items))
            if matches!(
                items.as_slice(),
                [BrowserValue::Number(2), BrowserValue::Number(3)]
            )
        ));
    }
}
