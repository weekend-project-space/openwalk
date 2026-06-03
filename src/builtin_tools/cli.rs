use anyhow::Result;
use scheme4r::Value;

use super::catalog::{builtin_tool_spec_for, BuiltinCliArgParser};

pub fn cli_args_to_scheme_values(name: &str, args: &[String]) -> Result<Vec<Value>> {
    let parser = builtin_tool_spec_for(name)
        .map(|spec| spec.cli_args)
        .unwrap_or(BuiltinCliArgParser::Strings);

    match parser {
        BuiltinCliArgParser::BrowserOpen => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_browser_open_arg(name, index, value))
            .collect(),
        BuiltinCliArgParser::Number { label } => args
            .iter()
            .map(|value| cli_number_arg(name, value, label))
            .collect(),
        BuiltinCliArgParser::XyNumbers => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_number_arg(name, value, cli_xy_label(index)))
            .collect(),
        BuiltinCliArgParser::MouseDownUp => args
            .iter()
            .enumerate()
            .map(|(index, value)| {
                if index < 2 {
                    cli_number_arg(name, value, cli_xy_label(index))
                } else {
                    Ok(Value::string(value.clone()))
                }
            })
            .collect(),
        BuiltinCliArgParser::MouseWheel => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_number_arg(name, value, cli_mouse_wheel_label(index)))
            .collect(),
        BuiltinCliArgParser::Strings => Ok(args.iter().cloned().map(Value::string).collect()),
    }
}

fn cli_browser_open_arg(name: &str, index: usize, raw: &str) -> Result<Value> {
    if index != 1 {
        return Ok(Value::string(raw.to_string()));
    }

    match raw {
        "true" => Ok(Value::Boolean(true)),
        "false" => Ok(Value::Boolean(false)),
        _ => Err(anyhow::anyhow!(
            "`{name}` expected `new-tab` to be a boolean, got `{raw}`"
        )),
    }
}

fn cli_number_arg(name: &str, raw: &str, label: &str) -> Result<Value> {
    let value = raw.parse::<i64>().map_err(|_| {
        anyhow::anyhow!("`{name}` expected `{label}` to be an integer, got `{raw}`")
    })?;
    Ok(Value::Number(value))
}

fn cli_xy_label(index: usize) -> &'static str {
    match index {
        0 => "x",
        1 => "y",
        _ => "value",
    }
}

fn cli_mouse_wheel_label(index: usize) -> &'static str {
    match index {
        0 => "x",
        1 => "y",
        2 => "delta-x",
        3 => "delta-y",
        _ => "value",
    }
}
