use scheme4r::Value;

use super::cli_args_to_scheme_values;

#[test]
fn browser_open_cli_args_convert_new_tab_to_boolean() {
    let values = cli_args_to_scheme_values(
        "browser-open",
        &["https://example.com".to_string(), "true".to_string()],
    )
    .expect("browser-open args should convert");

    assert_eq!(values.len(), 2);
    match &values[0] {
        Value::String(value) => assert_eq!(value.to_plain_string(), "https://example.com"),
        other => panic!("url should remain a string, got {other:?}"),
    }
    match values[1] {
        Value::Boolean(value) => assert!(value),
        ref other => panic!("new-tab should be a boolean, got {other:?}"),
    }
}

#[test]
fn browser_open_cli_args_reject_invalid_new_tab_boolean() {
    let error = cli_args_to_scheme_values(
        "browser-open",
        &["https://example.com".to_string(), "yes".to_string()],
    )
    .expect_err("invalid new-tab boolean should fail");

    assert!(error.to_string().contains("expected `new-tab`"));
}
