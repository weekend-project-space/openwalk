use anyhow::{bail, Context, Result};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Yaml,
    Md,
    Json,
}

pub fn parse_output_format(raw: &str) -> Result<OutputFormat> {
    let value = raw.trim();
    match value {
        "yaml" => Ok(OutputFormat::Yaml),
        "md" => Ok(OutputFormat::Md),
        "json" => Ok(OutputFormat::Json),
        _ => bail!("unsupported output format `{value}`. Supported formats: yaml, md, json"),
    }
}

pub fn normalize_result_value(display: &str) -> JsonValue {
    if display == "#t" {
        return JsonValue::Bool(true);
    }
    if display == "#f" {
        return JsonValue::Bool(false);
    }

    if let Ok(value) = serde_json::from_str::<JsonValue>(display) {
        if let JsonValue::String(inner) = value {
            if let Ok(parsed_inner) = serde_json::from_str::<JsonValue>(&inner) {
                return parsed_inner;
            }
            return JsonValue::String(inner);
        }
        return value;
    }

    JsonValue::String(display.to_string())
}

pub fn print_execution_result(format: OutputFormat, payload: &JsonValue) -> Result<()> {
    match format {
        OutputFormat::Yaml => {
            let yaml =
                serde_yaml::to_string(payload).context("failed to serialize execution output")?;
            print!("{yaml}");
        }
        OutputFormat::Md => print_markdown_execution_result(payload)?,
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(payload)
                .context("failed to serialize execution output")?;
            println!("{json}");
        }
    }

    Ok(())
}

fn print_markdown_execution_result(payload: &JsonValue) -> Result<()> {
    let mut out = String::new();
    out.push_str("# Execution Result\n\n");

    let mut has_summary = false;
    for key in ["mode", "tool", "script", "source", "status"] {
        if let Some(value) = payload.get(key) {
            has_summary = true;
            out.push_str("- ");
            out.push_str(key);
            out.push_str(": ");
            out.push_str(inline_markdown_value(value).as_str());
            out.push('\n');
        }
    }
    if has_summary {
        out.push('\n');
    }

    if let Some(args) = payload.get("args") {
        out.push_str("## Args\n\n");
        out.push_str("```yaml\n");
        out.push_str(yaml_body(args)?.as_str());
        out.push_str("```\n\n");
    }

    if let Some(result) = payload.get("result") {
        out.push_str("## Result\n\n");
        out.push_str("```yaml\n");
        out.push_str(yaml_body(result)?.as_str());
        out.push_str("```\n");
    } else if !has_summary {
        out.push_str("```yaml\n");
        out.push_str(yaml_body(payload)?.as_str());
        out.push_str("```\n");
    }

    print!("{out}");
    Ok(())
}

fn inline_markdown_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(_) | JsonValue::Number(_) => value.to_string(),
        JsonValue::String(text) => text.clone(),
        JsonValue::Array(_) | JsonValue::Object(_) => "(see section below)".to_string(),
    }
}

fn yaml_body(value: &JsonValue) -> Result<String> {
    let mut yaml = serde_yaml::to_string(value).context("failed to serialize execution output")?;
    if let Some(rest) = yaml.strip_prefix("---\n") {
        yaml = rest.to_string();
    }
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    Ok(yaml)
}
