use anyhow::Result;
use scheme4r::Value;

pub fn cli_args_to_scheme_values(name: &str, args: &[String]) -> Result<Vec<Value>> {
    match name {
        "browser-open" => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_browser_open_arg(name, index, value))
            .collect(),
        "time-sleep" => args
            .iter()
            .map(|value| cli_number_arg(name, value, "ms"))
            .collect(),
        "inspect-pick" => args
            .iter()
            .map(|value| cli_number_arg(name, value, "timeout-ms"))
            .collect(),
        "page-scroll-to" | "page-scroll-by" | "browser-resize" => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_number_arg(name, value, cli_xy_label(index)))
            .collect(),
        "mouse-move" | "mouse-click" | "touch-tap" => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_number_arg(name, value, cli_xy_label(index)))
            .collect(),
        "mouse-down" | "mouse-up" => args
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
        "mouse-wheel" => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_number_arg(name, value, cli_mouse_wheel_label(index)))
            .collect(),
        _ => Ok(args.iter().cloned().map(Value::string).collect()),
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
