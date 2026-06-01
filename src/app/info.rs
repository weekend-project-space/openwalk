use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::{
    builtin_tools,
    output::{parse_output_format, print_execution_result, OutputFormat},
    tool_metadata::{load_tool_metadata, ToolArgument},
    workspace::{GlobalHome, Workspace},
};

use super::{
    presentation::{tool_usage_from_metadata, trim_tool_description},
    target::{
        package_exists, resolve_global_tool_target, resolve_kit_tool_target, resolve_script_target,
        resolve_workspace_tool_target, tool_exists,
    },
    types::{ToolInfoEntry, ToolInfoView},
};

pub(super) fn show_tool_info(
    workspace: &Workspace,
    global_home: &GlobalHome,
    target: String,
    format: String,
) -> Result<()> {
    let output_format = parse_output_format(&format)?;
    let info = load_tool_info(workspace, global_home, &target)?;
    let view = build_tool_info_view(&info, &target);
    print_tool_info_view(output_format, &view)
}

pub(super) fn build_tool_info_view(info: &ToolInfoEntry, target: &str) -> ToolInfoView {
    let metadata = &info.metadata;
    let usage_name = if info.source == "script-path" {
        target
    } else {
        metadata.name.as_str()
    };

    ToolInfoView {
        name: metadata.name.clone(),
        usage: tool_usage_from_metadata(usage_name, &metadata.args),
        description: trim_tool_description(metadata.description.as_str()),
        source: display_tool_source(info.source.as_str()),
        script: info.script.clone(),
        args: metadata.args.clone(),
        options: tool_info_options(info.source.as_str(), metadata.name.as_str()),
        returns: metadata.returns.clone(),
        examples: metadata.examples.clone(),
        domains: metadata.domains.clone(),
        read_only: metadata.read_only,
        requires_login: metadata.requires_login,
        tags: metadata.tags.clone(),
    }
}

fn display_tool_source(source: &str) -> String {
    match source {
        "host-function" => "builtin".to_string(),
        "workspace-tool" => "workspace".to_string(),
        "global-tool" => "global".to_string(),
        "script-path" => "script".to_string(),
        _ => source.to_string(),
    }
}

pub(super) fn print_tool_info_view(format: OutputFormat, view: &ToolInfoView) -> Result<()> {
    match format {
        OutputFormat::Json => print_execution_result(format, &serde_json::to_value(view)?),
        OutputFormat::Text | OutputFormat::Yaml => {
            print!("{}", render_tool_help_text(view));
            Ok(())
        }
        OutputFormat::Md => {
            print!("{}", render_tool_help_markdown(view));
            Ok(())
        }
    }
}

pub(super) fn render_tool_help_text(view: &ToolInfoView) -> String {
    let mut out = String::new();
    out.push_str(view.name.as_str());
    out.push_str("\n\n");
    out.push_str(view.description.as_str());
    out.push_str("\n\n");

    out.push_str("Usage:\n");
    out.push_str("  openwalk exec ");
    out.push_str(view.usage.as_str());
    out.push_str("\n");

    push_argument_section(&mut out, "Arguments", &view.args);
    push_argument_section(&mut out, "Options", &view.options);

    out.push_str("\nReturns:\n");
    out.push_str("  ");
    out.push_str(view.returns.return_type.as_str());
    if !view.returns.description.is_empty() {
        out.push_str("  ");
        out.push_str(view.returns.description.as_str());
    }
    out.push('\n');

    if !view.examples.is_empty() {
        out.push_str("\nExamples:\n");
        for example in &view.examples {
            out.push_str("  ");
            out.push_str(example.as_str());
            out.push('\n');
        }
    }

    out
}

pub(super) fn render_tool_help_markdown(view: &ToolInfoView) -> String {
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(view.name.as_str());
    out.push_str("\n\n");
    out.push_str(view.description.as_str());
    out.push_str("\n\n");
    out.push_str("## Usage\n\n");
    out.push_str("```text\n");
    out.push_str("openwalk exec ");
    out.push_str(view.usage.as_str());
    out.push_str("\n```\n");

    push_markdown_argument_section(&mut out, "Arguments", &view.args);
    push_markdown_argument_section(&mut out, "Options", &view.options);

    out.push_str("\n## Returns\n\n");
    out.push_str("- `");
    out.push_str(view.returns.return_type.as_str());
    out.push_str("`: ");
    out.push_str(view.returns.description.as_str());
    out.push('\n');

    if !view.examples.is_empty() {
        out.push_str("\n## Examples\n\n");
        out.push_str("```text\n");
        for example in &view.examples {
            out.push_str(example.as_str());
            out.push('\n');
        }
        out.push_str("```\n");
    }

    out
}

fn push_argument_section(out: &mut String, title: &str, args: &[ToolArgument]) {
    if args.is_empty() {
        return;
    }

    out.push('\n');
    out.push_str(title);
    out.push_str(":\n");
    let name_width = args
        .iter()
        .map(|arg| help_arg_label(arg).chars().count())
        .max()
        .unwrap_or(0);
    for arg in args {
        let label = help_arg_label(arg);
        out.push_str("  ");
        out.push_str(label.as_str());
        out.push_str(&" ".repeat(name_width.saturating_sub(label.chars().count()) + 2));
        out.push_str(arg.description.as_str());
        out.push('\n');
    }
}

fn push_markdown_argument_section(out: &mut String, title: &str, args: &[ToolArgument]) {
    if args.is_empty() {
        return;
    }

    out.push_str("\n## ");
    out.push_str(title);
    out.push_str("\n\n");
    for arg in args {
        out.push_str("- `");
        out.push_str(help_arg_label(arg).as_str());
        out.push_str("`: ");
        out.push_str(arg.description.as_str());
        out.push('\n');
    }
}

fn help_arg_label(arg: &ToolArgument) -> String {
    if arg.name.starts_with('-') {
        arg.name.clone()
    } else if arg.required {
        format!("<{}>", arg.name)
    } else {
        format!("[{}]", arg.name)
    }
}

fn tool_info_options(source: &str, name: &str) -> Vec<ToolArgument> {
    let mut options = Vec::new();

    if source == "host-function" && name != "browser-list" && is_browser_tool_name(name) {
        options.push(tool_option(
            "-s, --session <name>",
            "string",
            "指定浏览器会话名称",
        ));
    }

    if name == "browser-open" {
        options.push(tool_option("--headed", "bool", "以有界面模式打开浏览器"));
        options.push(tool_option("--new-tab", "bool", "在现有会话中打开新标签页"));
        options.push(tool_option(
            "--profile <path>",
            "string",
            "指定浏览器 profile 目录",
        ));
    }

    options
}

fn tool_option(name: &str, arg_type: &str, description: &str) -> ToolArgument {
    ToolArgument {
        name: name.to_string(),
        arg_type: arg_type.to_string(),
        required: false,
        default: None,
        description: description.to_string(),
    }
}

fn is_browser_tool_name(name: &str) -> bool {
    name != "openwalk-output-format"
        && (name.starts_with("browser-")
            || name.starts_with("page-")
            || name.starts_with("element-")
            || name.starts_with("keyboard-")
            || name.starts_with("mouse-")
            || name.starts_with("touch-")
            || name.starts_with("tab-")
            || name.starts_with("network-")
            || name.starts_with("console")
            || name.starts_with("inspect-")
            || name.starts_with("tracing-")
            || name.starts_with("js-")
            || name.starts_with("time-")
            || name.starts_with("cdp-"))
}

pub(super) fn load_tool_info(
    workspace: &Workspace,
    global_home: &GlobalHome,
    target: &str,
) -> Result<ToolInfoEntry> {
    if let Some(script_path) = resolve_script_target(target)? {
        return build_tool_info("script-path", script_path);
    }

    if let Some(script_path) = resolve_workspace_tool_target(workspace, target)? {
        return build_tool_info("workspace-tool", script_path);
    }

    if let Some(script_path) = resolve_global_tool_target(global_home, target)? {
        return build_tool_info("global-tool", script_path);
    }

    if let Some(script_path) = resolve_kit_tool_target(global_home, target)? {
        return build_tool_info("kit-tool", script_path);
    }

    if tool_exists(target) {
        return build_builtin_tool_info(target);
    }

    if package_exists(&workspace.load_tools_or_default()?, target) {
        bail!(
            "tool `{target}` is registered in the workspace, but no script entry was found at {}",
            workspace.tool_entry_path(target).display()
        );
    }

    if package_exists(&global_home.load_tools()?, target) {
        bail!(
            "tool `{target}` is registered globally, but no script entry was found at {}",
            global_home.tool_entry_path(target).display()
        );
    }

    bail!(
        "tool `{target}` was not found. Pass a local script path like `./demo.scm` or an installed tool ref"
    );
}

fn build_tool_info(source: &str, script_path: PathBuf) -> Result<ToolInfoEntry> {
    let metadata = load_tool_metadata(&script_path)?;
    Ok(ToolInfoEntry {
        name: metadata.name.clone(),
        source: source.to_string(),
        script: Some(script_path.display().to_string()),
        metadata,
    })
}

pub(super) fn build_builtin_tool_info(name: &str) -> Result<ToolInfoEntry> {
    let metadata = builtin_tools::builtin_tool_metadata(name)
        .ok_or_else(|| anyhow::anyhow!("builtin host function `{name}` was not found"))?;
    Ok(ToolInfoEntry {
        name: metadata.name.clone(),
        source: "host-function".to_string(),
        script: None,
        metadata,
    })
}
