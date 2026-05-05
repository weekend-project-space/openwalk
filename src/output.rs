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

/// -------------------------
/// 统一格式化入口（纯函数）
/// -------------------------
pub fn format_execution_result(format: OutputFormat, payload: &JsonValue) -> Result<String> {
    match format {
        OutputFormat::Yaml => {
            let yaml =
                serde_yaml::to_string(payload).context("failed to serialize execution output")?;
            Ok(yaml)
        }
        OutputFormat::Md => format_markdown_execution_result(payload),
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(payload)
                .context("failed to serialize execution output")?;
            Ok(json)
        }
    }
}

/// -------------------------
/// 保留原行为：print 只是包装
/// -------------------------
pub fn print_execution_result(format: OutputFormat, payload: &JsonValue) -> Result<()> {
    let out = format_execution_result(format, payload)?;
    print!("{out}");
    Ok(())
}

/// -------------------------
/// Markdown 格式化（纯函数）
/// -------------------------
fn format_markdown_execution_result(payload: &JsonValue) -> Result<String> {
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

    Ok(out)
}

/// -------------------------
/// 值的 markdown 内联格式化
/// -------------------------
fn inline_markdown_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(_) | JsonValue::Number(_) => value.to_string(),
        JsonValue::String(text) => text.clone(),
        JsonValue::Array(_) | JsonValue::Object(_) => "(see section below)".to_string(),
    }
}

/// -------------------------
/// YAML body 统一处理
/// -------------------------
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

/// -------------------------
/// tests
/// -------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_md_snapshot() {
        let input = json!({
            "mode": "test",
            "tool": "demo",
            "status": "ok",
            "args": {"a": 1},
            "result": {"b": 2}
        });

        let md = format_execution_result(OutputFormat::Md, &input).unwrap();
        assert!(md.contains("# Execution Result"));
        assert!(md.contains("## Args"));
        assert!(md.contains("## Result"));
    }
}
