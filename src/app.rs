use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::json;

use crate::{
    browser::{
        attach_browser_session_with_options, ensure_browser_session_with_options, BrowserService,
        BrowserSessionLaunchOptions, EphemeralLaunchOptions,
    },
    cli::{Cli, Command, ToolCommand, ToolExecArgs},
    output::{normalize_result_value, parse_output_format, print_execution_result, OutputFormat},
    scheme_runtime,
    tool_metadata::{load_tool_metadata, ToolMetadata},
    workspace::{GlobalHome, InstalledPackage, Workspace},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// These names are available inside Scheme scripts as host-provided browser functions.
const BUILTIN_TOOLS: &[&str] = scheme_runtime::SCHEME_BUILTINS;

#[derive(Debug, Clone, Serialize)]
// Small response object used by `tool list --json`.
struct ToolListEntry {
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct ToolInfoEntry {
    name: String,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    script: Option<String>,
    metadata: ToolMetadata,
}

#[derive(Debug, Clone)]
struct RuntimeInvocationArgs {
    runtime_args: Vec<String>,
    session: Option<String>,
    output_format: OutputFormat,
}

#[derive(Debug, Clone)]
struct BrowserLaunchOptions {
    session: Option<String>,
    headless: Option<bool>,
    profile: Option<PathBuf>,
    create_session_if_missing: bool,
}

impl BrowserLaunchOptions {
    fn with_session(session: Option<String>) -> Self {
        Self {
            session,
            ..Self::default()
        }
    }
}

impl Default for BrowserLaunchOptions {
    fn default() -> Self {
        Self {
            session: None,
            headless: None,
            profile: None,
            create_session_if_missing: true,
        }
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    // The app layer owns command dispatch so CLI parsing, persistence, and execution policy
    // stay separated.
    match cli.command {
        Command::Init => {
            let workspace = Workspace::discover()?;
            init_workspace(&workspace)
        }
        Command::Run(args) => run_local(args).await,
        Command::Exec(args) => {
            let workspace = Workspace::discover()?;
            let global_home = GlobalHome::discover()?;
            exec_tool(&workspace, &global_home, args).await
        }
        Command::Tool { command } => {
            let workspace = Workspace::discover()?;
            let global_home = GlobalHome::discover()?;
            handle_tool_command(&workspace, &global_home, command)
        }
    }
}

fn init_workspace(workspace: &Workspace) -> Result<()> {
    let summary = workspace.init()?;

    println!("workspace: {}", workspace.base_dir().display());
    println!(
        "status: {}",
        if summary.created_root || summary.created_manifest || summary.created_tool_dir {
            "initialized"
        } else {
            "already initialized"
        }
    );
    println!("created_root: {}", summary.created_root);
    println!("created_manifest: {}", summary.created_manifest);
    println!("created_tool_dir: {}", summary.created_tool_dir);
    println!("manifest: {}", workspace.manifest_path().display());

    Ok(())
}

async fn run_local(args: ToolExecArgs) -> Result<()> {
    let ToolExecArgs {
        tool,
        args: cli_args,
    } = args;

    let workspace = Workspace::discover()?;
    let global_home = GlobalHome::discover()?;
    let script_path = if let Some(path) = resolve_script_target(&tool)? {
        path
    } else {
        if tool_exists(&tool) {
            bail!(
                "`openwalk run` only runs Scheme scripts and workspace tools. `{}` is a built-in host function. Use `openwalk exec {}` instead, or call it inside a .scm script.",
                tool,
                tool
            );
        }
        workspace.ensure_initialized()?;
        resolve_run_target(&workspace, &tool)?
    };

    run_scheme_script(&global_home, "run", &script_path, &cli_args).await
}

async fn exec_tool(
    workspace: &Workspace,
    global_home: &GlobalHome,
    args: ToolExecArgs,
) -> Result<()> {
    let ToolExecArgs {
        tool,
        args: cli_args,
    } = args;

    if let Some(script_path) = resolve_script_target(&tool)? {
        return run_scheme_script(global_home, "exec", &script_path, &cli_args).await;
    }

    if let Some(script_path) = resolve_workspace_tool_target(workspace, &tool) {
        return run_scheme_script(global_home, "exec", &script_path, &cli_args).await;
    }

    if tool_exists(&tool) {
        return run_builtin_tool(global_home, "exec", &tool, &cli_args).await;
    }

    let parsed_args = extract_common_runtime_args(&cli_args)?;

    // `exec` keeps the future local-first / remote-fallback decision visible even before
    // package manifests and remote runtime resolution are wired in.
    let source = if package_exists(&workspace.load_tools_or_default()?, &tool) {
        "workspace-package"
    } else if package_exists(&global_home.load_tools()?, &tool) {
        "global-package"
    } else {
        "remote-fallback"
    };

    print_execution_result(
        parsed_args.output_format,
        &json!({
            "mode": "exec",
            "tool": tool,
            "source": source,
            "args": parsed_args.runtime_args,
            "status": "simulated",
        }),
    )?;

    Ok(())
}

fn handle_tool_command(
    workspace: &Workspace,
    global_home: &GlobalHome,
    command: ToolCommand,
) -> Result<()> {
    match command {
        ToolCommand::Add { package } => install_package(workspace, package),
        ToolCommand::Remove { package } => uninstall_package(workspace, package),
        ToolCommand::Install { package } => install_global_package(global_home, package),
        ToolCommand::Uninstall { package } => uninstall_global_package(global_home, package),
        ToolCommand::List { json } => list_tools(workspace, global_home, json),
        ToolCommand::Info { tool, json } => show_tool_info(workspace, global_home, tool, json),
    }
}

fn install_package(workspace: &Workspace, package: String) -> Result<()> {
    let mut store = workspace.load_tools()?;

    // Phase 1 still models installs as exact package-name entries in the local tool store.
    if store.packages.iter().any(|item| item.name == package) {
        println!("package: {package}");
        println!("status: already installed");
        return Ok(());
    }

    store.packages.push(InstalledPackage {
        name: package.clone(),
        version: None,
        path: None,
    });
    store
        .packages
        .sort_by(|left, right| left.name.cmp(&right.name));
    workspace.save_tools(&store)?;

    println!("package: {package}");
    println!("status: installed");

    Ok(())
}

fn uninstall_package(workspace: &Workspace, package: String) -> Result<()> {
    let mut store = workspace.load_tools()?;
    let original_len = store.packages.len();
    store.packages.retain(|item| item.name != package);

    if store.packages.len() == original_len {
        bail!("package `{package}` is not installed");
    }

    workspace.save_tools(&store)?;

    println!("package: {package}");
    println!("status: uninstalled");

    Ok(())
}

fn install_global_package(global_home: &GlobalHome, package: String) -> Result<()> {
    let mut store = global_home.load_tools()?;
    let already_installed = package_exists(&store, &package);

    if !already_installed {
        store.packages.push(InstalledPackage {
            name: package.clone(),
            version: None,
            path: None,
        });
        store
            .packages
            .sort_by(|left, right| left.name.cmp(&right.name));
        global_home.save_tools(&store)?;
    } else {
        global_home.init()?;
    }

    let shim_path = write_global_shim(global_home, &package)?;

    println!("package: {package}");
    println!("scope: global");
    println!(
        "status: {}",
        if already_installed {
            "already installed"
        } else {
            "installed"
        }
    );
    println!("shim: {}", shim_path.display());
    println!("bin_dir: {}", global_home.bin_dir().display());
    println!(
        "hint: add {} to PATH to run `{package}` directly",
        global_home.bin_dir().display()
    );

    Ok(())
}

fn uninstall_global_package(global_home: &GlobalHome, package: String) -> Result<()> {
    let mut store = global_home.load_tools()?;
    let original_len = store.packages.len();
    store.packages.retain(|item| item.name != package);

    if store.packages.len() == original_len {
        bail!("package `{package}` is not globally installed");
    }

    global_home.save_tools(&store)?;

    let shim_path = global_home.shim_path(&package);
    if shim_path.exists() {
        fs::remove_file(&shim_path)
            .with_context(|| format!("failed to remove shim {}", shim_path.display()))?;
    }

    println!("package: {package}");
    println!("scope: global");
    println!("status: uninstalled");
    println!("shim: {}", shim_path.display());

    Ok(())
}

fn list_tools(workspace: &Workspace, global_home: &GlobalHome, json: bool) -> Result<()> {
    let workspace_store = workspace.load_tools_or_default()?;
    let local_tools = workspace_tool_entries(workspace)?;
    let global_store = global_home.load_tools()?;

    if json {
        let mut entries = builtin_entries();
        entries.extend(local_tools.iter().cloned());
        entries.extend(
            workspace_store
                .packages
                .iter()
                .map(|package| ToolListEntry {
                    name: package.name.clone(),
                    kind: "declared-tool".to_string(),
                    description: None,
                    script: None,
                    version: package.version.clone(),
                    path: package.path.clone(),
                }),
        );
        entries.extend(global_store.packages.iter().map(|package| ToolListEntry {
            name: package.name.clone(),
            kind: "global-package".to_string(),
            description: None,
            script: None,
            version: package.version.clone(),
            path: package.path.clone(),
        }));

        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    println!("scheme_host_functions:");
    for tool in BUILTIN_TOOLS {
        println!("  - {tool}");
    }

    println!("workspace_tools:");
    if local_tools.is_empty() {
        println!("  - (none)");
    } else {
        for tool in local_tools {
            let description = tool
                .description
                .as_deref()
                .map(|value| format!(" | {value}"))
                .unwrap_or_default();
            let script = tool.script.as_deref().unwrap_or_default();
            println!("  - {}{} ({script})", tool.name, description);
        }
    }

    println!("declared_tools:");
    if workspace_store.packages.is_empty() {
        println!("  - (none)");
    } else {
        for package in workspace_store.packages {
            let version = package
                .version
                .as_ref()
                .map(|value| format!(" version={value}"))
                .unwrap_or_default();
            let path = package
                .path
                .as_ref()
                .map(|value| format!(" path={value}"))
                .unwrap_or_default();
            println!("  - {}{}{}", package.name, version, path);
        }
    }

    println!("global_packages:");
    if global_store.packages.is_empty() {
        println!("  - (none)");
    } else {
        for package in global_store.packages {
            println!("  - {}", package.name);
        }
    }

    println!("global_bin_dir: {}", global_home.bin_dir().display());

    Ok(())
}

fn show_tool_info(
    workspace: &Workspace,
    global_home: &GlobalHome,
    target: String,
    json: bool,
) -> Result<()> {
    let info = load_tool_info(workspace, global_home, &target)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }

    println!("name: {}", info.name);
    println!("source: {}", info.source);
    println!("script: {}", info.script.as_deref().unwrap_or("(builtin)"));
    println!("description: {}", info.metadata.description);
    println!("read_only: {}", info.metadata.read_only);
    println!("requires_login: {}", info.metadata.requires_login);

    println!("domains:");
    if info.metadata.domains.is_empty() {
        println!("  - (none)");
    } else {
        for domain in &info.metadata.domains {
            println!("  - {domain}");
        }
    }

    println!("tags:");
    if info.metadata.tags.is_empty() {
        println!("  - (none)");
    } else {
        for tag in &info.metadata.tags {
            println!("  - {tag}");
        }
    }

    println!("args:");
    if info.metadata.args.is_empty() {
        println!("  - (none)");
    } else {
        for arg in &info.metadata.args {
            let default = arg
                .default
                .as_ref()
                .map(|value| format!(" | default={value}"))
                .unwrap_or_default();
            println!(
                "  - {} | type={} | required={}{} | description={}",
                arg.name, arg.arg_type, arg.required, default, arg.description
            );
        }
    }

    println!("returns:");
    println!("  type: {}", info.metadata.returns.return_type);
    println!("  description: {}", info.metadata.returns.description);

    println!("examples:");
    if info.metadata.examples.is_empty() {
        println!("  - (none)");
    } else {
        for example in &info.metadata.examples {
            println!("  - {example}");
        }
    }

    Ok(())
}

async fn run_scheme_script(
    global_home: &GlobalHome,
    mode: &str,
    script_path: &Path,
    args: &[String],
) -> Result<()> {
    let parsed_args = extract_common_runtime_args(args)?;
    let launch_options = BrowserLaunchOptions::with_session(parsed_args.session.clone());
    let browser = create_browser_service(global_home, launch_options).await?;
    let result =
        scheme_runtime::execute_script(script_path, &parsed_args.runtime_args, browser.client())
            .await;
    let shutdown = browser.shutdown().await;

    let display = result?;
    shutdown?;
    print_execution_result(
        parsed_args.output_format,
        &json!({
            "mode": mode,
            "script": script_path.display().to_string(),
            "args": parsed_args.runtime_args,
            "result": normalize_result_value(display.as_str()),
            "status": "executed",
        }),
    )?;

    Ok(())
}

async fn run_builtin_tool(
    global_home: &GlobalHome,
    mode: &str,
    tool: &str,
    args: &[String],
) -> Result<()> {
    let parsed_args = extract_common_runtime_args(args)?;
    let mut launch_options = BrowserLaunchOptions::with_session(parsed_args.session.clone());
    let runtime_args = if tool == "browser-open" {
        let (args, open_options) = parse_browser_open_runtime_args(&parsed_args.runtime_args)?;
        if open_options.headless.is_some() {
            launch_options.headless = open_options.headless;
        }
        if open_options.profile.is_some() {
            launch_options.profile = open_options.profile;
        }
        args
    } else if tool == "browser-close" {
        parse_browser_close_runtime_args(&parsed_args.runtime_args)?;
        launch_options.create_session_if_missing = false;
        parsed_args.runtime_args.clone()
    } else if matches!(tool, "tab-list" | "tab-new" | "tab-select" | "tab-close") {
        // Tab tools require an existing browser session/page prepared by `browser-open`.
        launch_options.create_session_if_missing = false;
        parsed_args.runtime_args.clone()
    } else {
        parsed_args.runtime_args.clone()
    };
    let browser = create_browser_service(global_home, launch_options).await?;
    let result = scheme_runtime::execute_builtin(tool, &runtime_args, browser.client()).await;
    let shutdown = browser.shutdown().await;

    let display = result?;
    shutdown?;
    print_execution_result(
        parsed_args.output_format,
        &json!({
            "mode": mode,
            "tool": tool,
            "source": "local-host-function",
            "args": runtime_args,
            "result": normalize_result_value(display.as_str()),
            "status": "executed",
        }),
    )?;

    Ok(())
}

fn extract_common_runtime_args(args: &[String]) -> Result<RuntimeInvocationArgs> {
    let mut runtime_args = Vec::new();
    let mut session: Option<String> = None;
    let mut output_format = OutputFormat::default();
    let mut passthrough = false;
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if passthrough {
            runtime_args.push(arg.clone());
            index += 1;
            continue;
        }

        if arg == "--" {
            passthrough = true;
            index += 1;
            continue;
        }

        if arg == "-s" || arg == "--session" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow::anyhow!("expects a session name after `{arg}`"))?;
            if value.is_empty() {
                bail!("received an empty session name");
            }
            session = Some(value.clone());
            index += 2;
            continue;
        }

        if let Some(value) = arg.strip_prefix("-s=") {
            if value.is_empty() {
                bail!("received an empty session name");
            }
            session = Some(value.to_string());
            index += 1;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--session=") {
            if value.is_empty() {
                bail!("received an empty session name");
            }
            session = Some(value.to_string());
            index += 1;
            continue;
        }

        if arg == "-f" || arg == "--format" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow::anyhow!("expects a format name after `{arg}`"))?;
            output_format = parse_output_format(value.as_str())?;
            index += 2;
            continue;
        }

        if let Some(value) = arg.strip_prefix("-f=") {
            output_format = parse_output_format(value)?;
            index += 1;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--format=") {
            output_format = parse_output_format(value)?;
            index += 1;
            continue;
        }

        runtime_args.push(arg.clone());
        index += 1;
    }

    Ok(RuntimeInvocationArgs {
        runtime_args,
        session,
        output_format,
    })
}

fn parse_browser_open_runtime_args(args: &[String]) -> Result<(Vec<String>, BrowserLaunchOptions)> {
    let mut launch_options = BrowserLaunchOptions::default();
    let mut positional = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if arg == "--headed" {
            launch_options.headless = Some(false);
            index += 1;
            continue;
        }
        if arg == "--profile" {
            let value = args.get(index + 1).ok_or_else(|| {
                anyhow::anyhow!("`browser-open` expects a profile path after `--profile`")
            })?;
            if value.is_empty() {
                bail!("`browser-open` received an empty profile path");
            }
            launch_options.profile = Some(PathBuf::from(value));
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--profile=") {
            if value.is_empty() {
                bail!("`browser-open` received an empty profile path");
            }
            launch_options.profile = Some(PathBuf::from(value));
            index += 1;
            continue;
        }
        if arg == "--" {
            positional.extend(args.iter().skip(index + 1).cloned());
            break;
        }
        if arg.starts_with('-') {
            bail!(
                "`browser-open` does not support option `{arg}`. Supported options are `--headed` and `--profile`"
            );
        }
        positional.push(arg.clone());
        index += 1;
    }

    if positional.len() != 1 {
        bail!("`browser-open` expects exactly one url argument");
    }

    Ok((vec![positional.remove(0)], launch_options))
}

fn parse_browser_close_runtime_args(args: &[String]) -> Result<()> {
    if args.is_empty() {
        Ok(())
    } else {
        bail!("`browser-close` does not accept positional arguments")
    }
}

async fn create_browser_service(
    global_home: &GlobalHome,
    launch_options: BrowserLaunchOptions,
) -> Result<BrowserService> {
    if let Some(session_name) = launch_options.session.as_deref() {
        let session_options = BrowserSessionLaunchOptions {
            requested_headless: launch_options.headless,
            requested_profile_dir: launch_options.profile.clone(),
        };
        let handle = if launch_options.create_session_if_missing {
            ensure_browser_session_with_options(global_home, session_name, session_options).await?
        } else {
            attach_browser_session_with_options(global_home, session_name, session_options).await?
        };
        Ok(BrowserService::attach_session(handle))
    } else if launch_options.profile.is_none() && launch_options.headless.is_none() {
        Ok(BrowserService::spawn())
    } else {
        Ok(BrowserService::spawn_ephemeral(EphemeralLaunchOptions {
            profile_dir: launch_options.profile.clone(),
            headless: launch_options.headless,
        }))
    }
}

fn builtin_entries() -> Vec<ToolListEntry> {
    BUILTIN_TOOLS
        .iter()
        .map(|tool| ToolListEntry {
            name: (*tool).to_string(),
            kind: "host-function".to_string(),
            description: scheme_runtime::builtin_tool_metadata(tool)
                .map(|metadata| metadata.description),
            script: None,
            version: None,
            path: None,
        })
        .collect()
}

fn workspace_tool_entries(workspace: &Workspace) -> Result<Vec<ToolListEntry>> {
    let mut entries = workspace
        .local_tools()?
        .into_iter()
        .map(|tool| {
            let description = load_tool_metadata(&tool.entry_path)
                .ok()
                .map(|metadata| metadata.description);

            ToolListEntry {
                name: tool.name,
                kind: "workspace-tool".to_string(),
                description,
                script: Some(tool.entry_path.display().to_string()),
                version: None,
                path: None,
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn tool_exists(tool: &str) -> bool {
    BUILTIN_TOOLS.contains(&tool)
}

fn package_exists(store: &crate::workspace::ToolStore, package: &str) -> bool {
    store.packages.iter().any(|item| item.name == package)
}

fn resolve_workspace_tool_target(workspace: &Workspace, target: &str) -> Option<PathBuf> {
    let entry = workspace.tool_entry_path(target);
    if entry.exists() {
        Some(entry)
    } else {
        None
    }
}

fn load_tool_info(
    workspace: &Workspace,
    global_home: &GlobalHome,
    target: &str,
) -> Result<ToolInfoEntry> {
    if let Some(script_path) = resolve_script_target(target)? {
        return build_tool_info("script-path", script_path);
    }

    if let Some(script_path) = resolve_workspace_tool_target(workspace, target) {
        return build_tool_info("workspace-tool", script_path);
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
            "tool `{target}` is installed globally, but its local script metadata is not available yet"
        );
    }

    bail!("tool `{target}` was not found. Pass a local `.scm` path or workspace tool name");
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

fn build_builtin_tool_info(name: &str) -> Result<ToolInfoEntry> {
    let metadata = scheme_runtime::builtin_tool_metadata(name)
        .ok_or_else(|| anyhow::anyhow!("builtin host function `{name}` was not found"))?;
    Ok(ToolInfoEntry {
        name: metadata.name.clone(),
        source: "host-function".to_string(),
        script: None,
        metadata,
    })
}

fn write_global_shim(global_home: &GlobalHome, package: &str) -> Result<PathBuf> {
    global_home.init()?;
    let shim_path = global_home.shim_path(package);
    let openwalk_path = std::env::current_exe()
        .context("failed to determine the current `openwalk` executable path")?;

    #[cfg(windows)]
    let script = format!(
        "@echo off\r\n\"{}\" exec \"{}\" %*\r\n",
        openwalk_path.display(),
        package
    );

    #[cfg(not(windows))]
    let script = format!(
        "#!/usr/bin/env sh\nexec {} exec {} \"$@\"\n",
        shell_single_quote(&openwalk_path.to_string_lossy()),
        shell_single_quote(package),
    );

    fs::write(&shim_path, script)
        .with_context(|| format!("failed to write shim {}", shim_path.display()))?;

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&shim_path)
            .with_context(|| format!("failed to read shim metadata {}", shim_path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&shim_path, permissions)
            .with_context(|| format!("failed to mark shim executable {}", shim_path.display()))?;
    }

    Ok(shim_path)
}

#[cfg(not(windows))]
fn shell_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

fn resolve_script_target(target: &str) -> Result<Option<PathBuf>> {
    let candidate = PathBuf::from(target);

    if candidate.exists() {
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        bail!("script path `{target}` exists but is not a file");
    }

    if target.ends_with(".scm") || target.chars().any(std::path::is_separator) {
        bail!("scheme script `{target}` was not found");
    }

    Ok(None)
}

fn resolve_run_target(workspace: &Workspace, target: &str) -> Result<PathBuf> {
    if let Some(path) = resolve_script_target(target)? {
        return Ok(path);
    }

    if target.contains('.') && !target.contains(std::path::MAIN_SEPARATOR) {
        bail!(
            "`openwalk run` expects a local `.scm` path or a workspace tool name, got `{target}`"
        );
    }

    let entry = workspace.tool_entry_path(target);
    if entry.exists() {
        return Ok(entry);
    }

    bail!(
        "tool `{target}` was not found. Expected `{}`",
        entry.display()
    )
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use tokio::sync::Semaphore;

    use super::*;

    static CWD_LOCK: Semaphore = Semaphore::const_new(1);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos();
            let path =
                env::temp_dir().join(format!("openwalk-app-test-{}-{timestamp}", process::id()));
            fs::create_dir_all(&path).expect("test temp dir should be created");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn initialized_workspace() -> (TestDir, Workspace) {
        let sandbox = TestDir::new();
        // Tests construct the workspace from an isolated base dir instead of mutating the repo.
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        workspace.init().expect("workspace should initialize");
        (sandbox, workspace)
    }

    fn initialized_global_home() -> (TestDir, GlobalHome) {
        let sandbox = TestDir::new();
        let global_home = GlobalHome::from_root(sandbox.path.join("global-home"));
        global_home.init().expect("global home should initialize");
        (sandbox, global_home)
    }

    #[tokio::test]
    async fn run_local_executes_scheme_script() {
        let _cwd_guard = CWD_LOCK
            .acquire()
            .await
            .expect("cwd lock should be acquired");
        let sandbox = TestDir::new();
        let previous_dir = env::current_dir().expect("cwd should be readable");
        env::set_current_dir(&sandbox.path).expect("should change cwd for the test");

        let workspace = Workspace::discover().expect("workspace should resolve");
        workspace.init().expect("workspace should initialize");
        let script_path = sandbox.path.join("double.scm");
        fs::write(&script_path, "(+ 19 23)").expect("script should be written");

        let result = run_local(ToolExecArgs {
            tool: script_path.display().to_string(),
            args: Vec::new(),
        })
        .await;

        env::set_current_dir(previous_dir).expect("cwd should be restored");
        result.expect("script should run");
    }

    #[tokio::test]
    async fn run_local_resolves_workspace_tool_names() {
        let _cwd_guard = CWD_LOCK
            .acquire()
            .await
            .expect("cwd lock should be acquired");
        let sandbox = TestDir::new();
        let previous_dir = env::current_dir().expect("cwd should be readable");
        env::set_current_dir(&sandbox.path).expect("should change cwd for the test");

        let workspace = Workspace::discover().expect("workspace should resolve");
        workspace.init().expect("workspace should initialize");
        let tool_dir = workspace.tool_dir("smoke");
        fs::create_dir_all(&tool_dir).expect("tool dir should be created");
        fs::write(tool_dir.join("main.scm"), "(+ 20 22)").expect("script should be written");

        let result = run_local(ToolExecArgs {
            tool: "smoke".to_string(),
            args: Vec::new(),
        })
        .await;

        env::set_current_dir(previous_dir).expect("cwd should be restored");
        result.expect("workspace tool should run");
    }

    #[test]
    fn resolve_script_target_returns_file_paths() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("demo.scm");
        fs::write(&script_path, "(+ 1 2)").expect("script should be written");

        let resolved = resolve_script_target(script_path.to_str().expect("valid utf8 path"))
            .expect("resolution should succeed");

        assert_eq!(resolved, Some(script_path));
    }

    #[test]
    fn resolve_script_target_rejects_missing_scheme_files() {
        let err = resolve_script_target("./missing.scm").expect_err("missing script should fail");
        assert!(err.to_string().contains("was not found"));
    }

    #[test]
    fn resolve_run_target_maps_tool_name_to_workspace_entry() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        workspace.init().expect("workspace should initialize");

        let tool_dir = workspace.tool_dir("browser-smoke");
        fs::create_dir_all(&tool_dir).expect("tool dir should be created");
        let entry = tool_dir.join("main.scm");
        fs::write(&entry, "(+ 1 2)").expect("script should be written");

        let resolved =
            resolve_run_target(&workspace, "browser-smoke").expect("workspace tool should resolve");
        assert_eq!(resolved, entry);
    }

    #[test]
    fn install_and_uninstall_package_updates_store() {
        let (_sandbox, workspace) = initialized_workspace();

        install_package(&workspace, "browser-tools".to_string()).expect("install should succeed");
        let installed = workspace
            .load_tools()
            .expect("tools should load after install");
        assert_eq!(
            installed.packages,
            vec![InstalledPackage {
                name: "browser-tools".to_string(),
                version: None,
                path: None,
            }]
        );

        uninstall_package(&workspace, "browser-tools".to_string())
            .expect("uninstall should succeed");
        let after_uninstall = workspace
            .load_tools()
            .expect("tools should load after uninstall");
        assert!(after_uninstall.packages.is_empty());
    }

    #[test]
    fn handle_tool_add_and_remove_updates_store() {
        let (_workspace_sandbox, workspace) = initialized_workspace();
        let (_global_sandbox, global_home) = initialized_global_home();

        handle_tool_command(
            &workspace,
            &global_home,
            ToolCommand::Add {
                package: "browser-tools".to_string(),
            },
        )
        .expect("tool add should succeed");

        let installed = workspace
            .load_tools()
            .expect("tools should load after tool add");
        assert_eq!(
            installed.packages,
            vec![InstalledPackage {
                name: "browser-tools".to_string(),
                version: None,
                path: None,
            }]
        );

        handle_tool_command(
            &workspace,
            &global_home,
            ToolCommand::Remove {
                package: "browser-tools".to_string(),
            },
        )
        .expect("tool remove should succeed");

        let after_remove = workspace
            .load_tools()
            .expect("tools should load after tool remove");
        assert!(after_remove.packages.is_empty());
    }

    #[test]
    fn handle_tool_install_and_uninstall_updates_global_store_and_shim() {
        let (_workspace_sandbox, workspace) = initialized_workspace();
        let (_global_sandbox, global_home) = initialized_global_home();

        handle_tool_command(
            &workspace,
            &global_home,
            ToolCommand::Install {
                package: "browser-tools".to_string(),
            },
        )
        .expect("tool install should succeed");

        let installed = global_home
            .load_tools()
            .expect("global tools should load after tool install");
        assert_eq!(
            installed.packages,
            vec![InstalledPackage {
                name: "browser-tools".to_string(),
                version: None,
                path: None,
            }]
        );
        assert!(global_home.shim_path("browser-tools").exists());

        handle_tool_command(
            &workspace,
            &global_home,
            ToolCommand::Uninstall {
                package: "browser-tools".to_string(),
            },
        )
        .expect("tool uninstall should succeed");

        let after_uninstall = global_home
            .load_tools()
            .expect("global tools should load after tool uninstall");
        assert!(after_uninstall.packages.is_empty());
        assert!(!global_home.shim_path("browser-tools").exists());
    }

    #[test]
    fn load_tool_info_reads_workspace_script_metadata() {
        let (_sandbox, workspace) = initialized_workspace();
        let (_global_sandbox, global_home) = initialized_global_home();
        let tool_dir = workspace.tool_dir("bing-search");
        fs::create_dir_all(&tool_dir).expect("tool dir should be created");
        fs::write(
            tool_dir.join("main.scm"),
            r#"#| @meta
{
  "name": "bing-search",
  "description": "Bing 搜索",
  "args": [],
  "returns": {
    "type": "object",
    "description": "{ results[] }"
  },
  "examples": ["openwalk run bing-search -- \"Claude Code\" 10"],
  "domains": ["www.bing.com"],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["search"]
}
|#
(define (main args) "ok")
"#,
        )
        .expect("script should be written");

        let info =
            load_tool_info(&workspace, &global_home, "bing-search").expect("tool info should load");

        assert_eq!(info.name, "bing-search");
        assert_eq!(info.source, "workspace-tool");
        assert_eq!(info.metadata.description, "Bing 搜索");
    }

    #[test]
    fn workspace_tool_entries_include_metadata_description() {
        let (_sandbox, workspace) = initialized_workspace();
        let tool_dir = workspace.tool_dir("bing-search");
        fs::create_dir_all(&tool_dir).expect("tool dir should be created");
        fs::write(
            tool_dir.join("main.scm"),
            r#"#| @meta
{
  "name": "bing-search",
  "description": "Bing 搜索并返回结构化结果",
  "args": [],
  "returns": {
    "type": "object",
    "description": "{ results[] }"
  },
  "examples": ["openwalk run bing-search -- \"Claude Code\" 10"],
  "domains": ["www.bing.com"],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["search"]
}
|#
(define (main args) "ok")
"#,
        )
        .expect("script should be written");

        let entries = workspace_tool_entries(&workspace).expect("workspace tools should load");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "bing-search");
        assert_eq!(
            entries[0].description.as_deref(),
            Some("Bing 搜索并返回结构化结果")
        );
    }

    #[test]
    fn load_tool_info_reads_builtin_host_function_metadata() {
        let workspace_sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
        let (_global_sandbox, global_home) = initialized_global_home();

        let info = load_tool_info(&workspace, &global_home, "browser-open")
            .expect("builtin info should load");

        assert_eq!(info.name, "browser-open");
        assert_eq!(info.source, "host-function");
        assert_eq!(info.script, None);
        assert_eq!(info.metadata.description, "新开标签页并导航到指定 URL。");
    }

    #[test]
    fn load_tool_info_reads_direct_script_metadata() {
        let workspace_sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
        let (_global_sandbox, global_home) = initialized_global_home();
        let script_path = workspace_sandbox.path.join("demo.scm");
        fs::write(
            &script_path,
            r#"#| @meta
{
  "name": "demo",
  "description": "演示脚本",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk tool info ./demo.scm"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["demo"]
}
|#
(define (main args) "ok")
"#,
        )
        .expect("script should be written");

        let info = load_tool_info(
            &workspace,
            &global_home,
            script_path.to_str().expect("valid utf8 path"),
        )
        .expect("tool info should load");

        assert_eq!(info.name, "demo");
        assert_eq!(info.source, "script-path");
    }

    #[test]
    fn load_tool_info_rejects_missing_metadata_header() {
        let workspace_sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
        let (_global_sandbox, global_home) = initialized_global_home();
        let script_path = workspace_sandbox.path.join("demo.scm");
        fs::write(&script_path, "(define (main args) \"ok\")").expect("script should be written");

        let err = load_tool_info(
            &workspace,
            &global_home,
            script_path.to_str().expect("valid utf8 path"),
        )
        .expect_err("missing metadata should fail");

        assert!(err
            .to_string()
            .contains("missing a `#| @meta ... |#` header"));
    }

    #[tokio::test]
    async fn exec_tool_allows_remote_fallback_for_unknown_tools() {
        let (_workspace_sandbox, workspace) = initialized_workspace();
        let (_global_sandbox, global_home) = initialized_global_home();

        exec_tool(
            &workspace,
            &global_home,
            ToolExecArgs {
                tool: "remote.browser.open".to_string(),
                args: vec!["https://example.com".to_string()],
            },
        )
        .await
        .expect("exec should allow remote fallback");
    }

    #[tokio::test]
    async fn exec_tool_executes_builtin_host_function() {
        let workspace_sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
        let (_global_sandbox, global_home) = initialized_global_home();

        exec_tool(
            &workspace,
            &global_home,
            ToolExecArgs {
                tool: "time-sleep".to_string(),
                args: vec!["0".to_string()],
            },
        )
        .await
        .expect("exec should run builtin host functions");
    }

    #[tokio::test]
    async fn exec_tool_allows_global_packages_without_workspace_init() {
        let workspace_sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
        let (_global_sandbox, global_home) = initialized_global_home();

        install_global_package(&global_home, "browser-tools".to_string())
            .expect("global install should succeed");

        exec_tool(
            &workspace,
            &global_home,
            ToolExecArgs {
                tool: "browser-tools".to_string(),
                args: vec!["https://example.com".to_string()],
            },
        )
        .await
        .expect("exec should allow globally installed packages without a workspace");
    }

    #[tokio::test]
    async fn run_local_rejects_builtin_host_function_names() {
        let _cwd_guard = CWD_LOCK
            .acquire()
            .await
            .expect("cwd lock should be acquired");
        let sandbox = TestDir::new();
        let previous_dir = env::current_dir().expect("cwd should be readable");
        env::set_current_dir(&sandbox.path).expect("should change cwd for the test");

        let result = run_local(ToolExecArgs {
            tool: "browser-open".to_string(),
            args: vec!["https://example.com".to_string()],
        })
        .await;

        env::set_current_dir(previous_dir).expect("cwd should be restored");

        let err = result.expect_err("run should reject builtin host functions");
        assert!(err
            .to_string()
            .contains("Use `openwalk exec browser-open` instead"));
    }

    #[test]
    fn extract_common_runtime_args_parses_session_and_escape() {
        let parsed = extract_common_runtime_args(&[
            "https://example.com".to_string(),
            "-s=qa".to_string(),
            "--".to_string(),
            "--session=raw".to_string(),
        ])
        .expect("runtime args should parse");

        assert_eq!(parsed.session.as_deref(), Some("qa"));
        assert_eq!(parsed.output_format, OutputFormat::Yaml);
        assert_eq!(
            parsed.runtime_args,
            vec![
                "https://example.com".to_string(),
                "--session=raw".to_string()
            ]
        );
    }

    #[test]
    fn extract_common_runtime_args_parses_short_format_flag() {
        let parsed =
            extract_common_runtime_args(&["-f=md".to_string(), "https://example.com".to_string()])
                .expect("runtime args should parse with format");

        assert_eq!(parsed.output_format, OutputFormat::Md);
        assert_eq!(parsed.runtime_args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn extract_common_runtime_args_parses_json_format_flag() {
        let parsed = extract_common_runtime_args(&[
            "--format=json".to_string(),
            "https://example.com".to_string(),
        ])
        .expect("runtime args should parse with json format");

        assert_eq!(parsed.output_format, OutputFormat::Json);
        assert_eq!(parsed.runtime_args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn extract_common_runtime_args_rejects_unknown_format() {
        let error = extract_common_runtime_args(&["--format=toml".to_string()])
            .expect_err("unknown format should fail");
        assert!(error.to_string().contains("unsupported output format"));
    }

    #[test]
    fn parse_browser_open_runtime_args_reads_browser_flags() {
        let (args, options) = parse_browser_open_runtime_args(&[
            "https://example.com".to_string(),
            "--headed".to_string(),
            "--profile=/tmp/openwalk-profile".to_string(),
        ])
        .expect("browser-open args should parse");

        assert_eq!(args, vec!["https://example.com".to_string()]);
        assert_eq!(options.headless, Some(false));
        assert_eq!(
            options.profile,
            Some(PathBuf::from("/tmp/openwalk-profile"))
        );
    }

    #[test]
    fn parse_browser_close_runtime_args_rejects_positional_args() {
        let err = parse_browser_close_runtime_args(&["extra".to_string()])
            .expect_err("browser-close should reject positional args");

        assert!(err
            .to_string()
            .contains("does not accept positional arguments"));
    }
}
