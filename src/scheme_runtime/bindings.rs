use std::path::Path;

use anyhow::Result;
use scheme4r::{eval::Engine, runtime::EnvRef, SchemeError, SchemeString, Value};

use crate::{
    output::{format_execution_result, parse_output_format, OutputFormat},
    tool_metadata::ToolMetadata,
    value_codec::{self, tool_metadata_to_scheme},
};

use super::host_functions;

pub(super) fn install_openwalk_bindings(
    env: EnvRef,
    script_path: &Path,
    args: &[String],
    session_name: Option<&str>,
    script_meta: Option<&ToolMetadata>,
) {
    let mut env_ref = env.borrow_mut();

    env_ref.define(
        "openwalk-script-path",
        Value::string(script_path.display().to_string()),
    );
    env_ref.define(
        "openwalk-args",
        Value::list(args.iter().cloned().map(Value::string).collect()),
    );
    env_ref.define(
        "openwalk-session-name",
        match session_name {
            Some(name) => Value::string(name),
            None => Value::Boolean(false),
        },
    );
    env_ref.define(
        "openwalk-script-meta",
        match script_meta {
            Some(meta) => tool_metadata_to_scheme(meta),
            None => Value::Boolean(false),
        },
    );
    env_ref.define(
        "openwalk-output-format",
        Value::builtin("openwalk-output-format", output_format),
    );

    host_functions::register_browser_builtins(&mut env_ref);
}

fn output_format(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("display-format", args, 1, 2)?;
    let json = value_codec::scheme_value_to_json(&args[0]);
    let format = if args.len() == 2 {
        match &args[1] {
            Value::String(s) => s.to_plain_string(),
            _ => return Err(SchemeError::runtime("format type_error: yaml | md | json")),
        }
    } else {
        "yaml".to_string()
    };
    let fmt: OutputFormat =
        parse_output_format(&format).map_err(|e| SchemeError::runtime(e.to_string()))?;

    let output =
        format_execution_result(fmt, &json).map_err(|e| SchemeError::runtime(e.to_string()))?;

    Ok(Value::String(SchemeString::new(output)))
}

fn expect_arity_range(
    name: &str,
    args: &[Value],
    min: usize,
    max: usize,
) -> Result<(), SchemeError> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(SchemeError::arity(format!(
            "`{name}` expects between {min} and {max} argument(s), got {}",
            args.len()
        )))
    }
}
