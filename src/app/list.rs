use anyhow::Result;
use serde_json::json;

use crate::{
    builtin_tools,
    cli::ToolListSourceFilter,
    output::{parse_output_format, print_execution_result, OutputFormat},
    tool_metadata::load_tool_metadata,
    workspace::{GlobalHome, Workspace},
};

use super::{
    presentation::{compact_tool_description, tool_usage_from_metadata},
    target::KIT_TOOL_NAMESPACES,
    types::ToolListEntry,
};

pub(super) fn list_tools(
    workspace: &Workspace,
    global_home: &GlobalHome,
    format: String,
    source: Option<ToolListSourceFilter>,
) -> Result<()> {
    let output_format = parse_output_format(&format)?;
    let entries = collect_tool_entries(workspace, global_home, source)?;

    let payload = match output_format {
        OutputFormat::Text | OutputFormat::Md => {
            serde_json::to_value(render_tool_list_lines(&entries))?
        }
        OutputFormat::Yaml | OutputFormat::Json => serde_json::to_value(entries)?,
    };

    print_tool_output(output_format, "tool-list", None, payload)
}

pub(super) fn collect_tool_entries(
    workspace: &Workspace,
    global_home: &GlobalHome,
    source: Option<ToolListSourceFilter>,
) -> Result<Vec<ToolListEntry>> {
    let mut entries = builtin_entries();
    entries.extend(workspace_tool_entries(workspace)?);
    entries.extend(global_tool_entries(global_home)?);
    entries.extend(kit_tool_entries(global_home)?);
    Ok(filter_tool_entries(entries, source))
}

fn filter_tool_entries(
    entries: Vec<ToolListEntry>,
    source: Option<ToolListSourceFilter>,
) -> Vec<ToolListEntry> {
    let Some(source) = source else {
        return entries;
    };

    let expected = match source {
        ToolListSourceFilter::Builtin => "builtin",
        ToolListSourceFilter::Workspace => "workspace",
        ToolListSourceFilter::Global => "global",
        ToolListSourceFilter::Kit => "kit",
    };

    entries
        .into_iter()
        .filter(|entry| entry.source == expected)
        .collect()
}

fn builtin_entries() -> Vec<ToolListEntry> {
    builtin_tools::SCHEME_BUILTINS
        .iter()
        .map(|tool| ToolListEntry {
            name: (*tool).to_string(),
            usage: builtin_tools::builtin_tool_metadata(tool)
                .map(|metadata| tool_usage_from_metadata(metadata.name.as_str(), &metadata.args))
                .unwrap_or_else(|| (*tool).to_string()),
            description: builtin_tools::builtin_tool_metadata(tool)
                .map(|metadata| compact_tool_description(tool, metadata.description.as_str()))
                .unwrap_or_else(|| tool.to_string()),
            source: "builtin".to_string(),
            alias: Vec::new(),
        })
        .collect()
}

pub(super) fn workspace_tool_entries(workspace: &Workspace) -> Result<Vec<ToolListEntry>> {
    let mut entries = workspace
        .local_tools()?
        .into_iter()
        .map(|tool| {
            let metadata = load_tool_metadata(&tool.entry_path).ok();
            let usage = metadata
                .as_ref()
                .map(|metadata| tool_usage_from_metadata(tool.name.as_str(), &metadata.args))
                .unwrap_or_else(|| tool.name.clone());
            let description = metadata
                .as_ref()
                .map(|metadata| metadata.description.clone())
                .unwrap_or_else(|| "Workspace Scheme tool".to_string());

            ToolListEntry {
                name: tool.name,
                usage,
                description,
                source: "workspace".to_string(),
                alias: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn global_tool_entries(global_home: &GlobalHome) -> Result<Vec<ToolListEntry>> {
    let mut entries = global_home
        .local_tools()?
        .into_iter()
        .map(|tool| {
            let metadata = load_tool_metadata(&tool.entry_path).ok();
            let usage = metadata
                .as_ref()
                .map(|metadata| tool_usage_from_metadata(tool.name.as_str(), &metadata.args))
                .unwrap_or_else(|| tool.name.clone());
            let description = metadata
                .as_ref()
                .map(|metadata| metadata.description.clone())
                .unwrap_or_else(|| "Global Scheme tool".to_string());

            ToolListEntry {
                name: tool.name,
                usage,
                description,
                source: "global".to_string(),
                alias: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn kit_tool_entries(global_home: &GlobalHome) -> Result<Vec<ToolListEntry>> {
    let mut entries = global_home
        .kit_tools_in_namespaces(KIT_TOOL_NAMESPACES)?
        .into_iter()
        .map(|tool| {
            let metadata = load_tool_metadata(&tool.entry_path).ok();
            let usage = metadata
                .as_ref()
                .map(|metadata| tool_usage_from_metadata(tool.name.as_str(), &metadata.args))
                .unwrap_or_else(|| tool.name.clone());
            let alias = kit_alias(tool.name.as_str());
            let description = metadata
                .as_ref()
                .map(|metadata| metadata.description.clone())
                .unwrap_or_else(|| "Kit Scheme tool".to_string());

            ToolListEntry {
                name: tool.name,
                usage,
                description,
                source: "kit".to_string(),
                alias,
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

pub(super) fn render_tool_list_lines(entries: &[ToolListEntry]) -> Vec<String> {
    let source_width = entries
        .iter()
        .map(|entry| entry.source.chars().count())
        .max()
        .unwrap_or(0);
    let source_width = source_width.max("SOURCE".len());
    let usage_width = entries
        .iter()
        .map(|entry| display_tool_usage(entry).chars().count())
        .max()
        .unwrap_or(0);
    let usage_width = usage_width.max("USAGE".len());

    let mut lines = vec![format!(
        "{:<source_width$}  {:<usage_width$}  DESCRIPTION",
        "SOURCE",
        "USAGE",
        source_width = source_width,
        usage_width = usage_width
    )];

    lines.extend(entries.iter().map(|entry| {
        let usage = display_tool_usage(entry);
        if entry.description.is_empty() {
            format!(
                "{:<source_width$}  {:<usage_width$}",
                entry.source,
                usage,
                source_width = source_width,
                usage_width = usage_width
            )
        } else {
            format!(
                "{:<source_width$}  {:<usage_width$}  {}",
                entry.source,
                usage,
                entry.description,
                source_width = source_width,
                usage_width = usage_width
            )
        }
    }));
    lines
}

fn display_tool_usage(entry: &ToolListEntry) -> String {
    if let Some(alias) = entry.alias.first() {
        format!("{alias} ({})", entry.usage)
    } else {
        entry.usage.clone()
    }
}

fn kit_alias(tool_name: &str) -> Vec<String> {
    let Some(short_name) = tool_name.strip_prefix("sys/") else {
        return Vec::new();
    };

    vec![short_name.to_string()]
}

pub(super) fn print_tool_output(
    format: OutputFormat,
    mode: &str,
    tool: Option<&str>,
    payload: serde_json::Value,
) -> Result<()> {
    let output_payload = if format == OutputFormat::Md {
        let mut wrapper = json!({
            "mode": mode,
            "status": "ok",
            "result": payload,
        });
        if let Some(tool) = tool {
            wrapper["tool"] = json!(tool);
        }
        wrapper
    } else {
        payload
    };

    print_execution_result(format, &output_payload)
}
