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
    cli::{Cli, Command, InitArgs, ProjectInstallArgs, ToolCommand, ToolExecArgs},
    output::{normalize_result_value, parse_output_format, print_execution_result, OutputFormat},
    scheme_runtime,
    tool_hub::install_tool_from_hub,
    tool_metadata::{load_tool_metadata, ToolMetadata},
    workspace::{GlobalHome, InitOptions, InstalledPackage, Workspace},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageInstallStatus {
    Installed,
    AlreadyInstalled,
}

#[derive(Debug, Clone)]
struct PackageInstallResult {
    entry_path: PathBuf,
    status: PackageInstallStatus,
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
        Command::Init(args) => {
            let workspace = Workspace::discover()?;
            init_workspace(&workspace, args)
        }
        Command::Install(args) => {
            let workspace = Workspace::discover()?;
            install_workspace_tools(&workspace, args)
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

fn init_workspace(workspace: &Workspace, args: InitArgs) -> Result<()> {
    let output_format = parse_output_format(&args.format)?;
    let options = InitOptions {
        name: args.name,
        tools: args.tools,
        force: args.force,
    };

    let summary = workspace.init_with_options(options)?;

    let status = if summary.overwritten_manifest {
        "reinitialized"
    } else if summary.created_root || summary.created_manifest || summary.created_tool_dir {
        "initialized"
    } else {
        "already initialized"
    };

    let mut payload = json!({
        "mode": "init",
        "workspace": workspace.base_dir().display().to_string(),
        "manifest": workspace.manifest_path().display().to_string(),
        "status": status,
        "created": {
            "root": summary.created_root,
            "manifest": summary.created_manifest,
            "tool_dir": summary.created_tool_dir,
        },
        "overwritten_manifest": summary.overwritten_manifest,
    });

    if let Some(backup) = summary.backup_path {
        payload["backup"] = json!(backup.display().to_string());
    }

    print_execution_result(output_format, &payload)?;
    Ok(())
}

fn install_workspace_tools(workspace: &Workspace, args: ProjectInstallArgs) -> Result<()> {
    let _ = args;
    install_declared_workspace_tools(workspace)?;
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

    if let Some(script_path) = resolve_global_tool_target(global_home, &tool) {
        return run_scheme_script(global_home, "exec", &script_path, &cli_args).await;
    }

    let installed = ensure_workspace_package_installed(workspace, &tool)?;
    run_scheme_script(global_home, "exec", &installed.entry_path, &cli_args).await
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
    let install = ensure_workspace_package_installed(workspace, &package)?;

    println!("package: {package}");
    println!("scope: workspace");
    println!(
        "status: {}",
        match install.status {
            PackageInstallStatus::Installed => "installed",
            PackageInstallStatus::AlreadyInstalled => "already installed",
        }
    );
    println!("script: {}", install.entry_path.display());

    Ok(())
}

fn uninstall_package(workspace: &Workspace, package: String) -> Result<()> {
    remove_workspace_package(workspace, &package)?;

    println!("package: {package}");
    println!("status: uninstalled");
    println!("scope: workspace");

    Ok(())
}

fn install_named_workspace_tools(workspace: &Workspace, packages: &[String]) -> Result<()> {
    let mut installed = Vec::new();
    let mut already_installed = Vec::new();

    for package in packages {
        let result = ensure_workspace_package_installed(workspace, package)?;
        match result.status {
            PackageInstallStatus::Installed => installed.push(package.clone()),
            PackageInstallStatus::AlreadyInstalled => already_installed.push(package.clone()),
        }
    }

    println!("scope: workspace");
    println!("status: installed");
    println!("requested: {}", packages.len());
    println!("installed: {}", installed.len());
    println!("already_installed: {}", already_installed.len());
    println!("packages:");
    for package in packages {
        let state = if installed.iter().any(|item| item == package) {
            "installed"
        } else {
            "already installed"
        };
        println!("  - {package} | {state}");
    }

    Ok(())
}

fn install_declared_workspace_tools(workspace: &Workspace) -> Result<()> {
    ensure_workspace_manifest_available(workspace)?;
    let store = workspace.load_tools()?;
    let packages = store
        .packages
        .into_iter()
        .map(|package| package.name)
        .collect::<Vec<_>>();

    if packages.is_empty() {
        println!("scope: workspace");
        println!("status: nothing to install");
        println!("declared: 0");
        return Ok(());
    }

    install_named_workspace_tools(workspace, &packages)
}

fn ensure_workspace_manifest_available(workspace: &Workspace) -> Result<()> {
    if !workspace.manifest_path().exists() {
        bail!(
            "project manifest {} was not found. Run `openwalk init` first.",
            workspace.manifest_path().display()
        );
    }

    if !workspace.is_initialized() {
        workspace.init_with_options(InitOptions::default())?;
    }

    Ok(())
}

fn remove_workspace_package(workspace: &Workspace, package: &str) -> Result<()> {
    ensure_workspace_manifest_available(workspace)?;
    let mut store = workspace.load_tools()?;
    let original_len = store.packages.len();
    store.packages.retain(|item| item.name != package);
    let tool_dir = workspace.tool_dir(package);
    let had_files = tool_dir.exists();

    if store.packages.len() == original_len && !had_files {
        bail!("package `{package}` is not installed");
    }

    if store.packages.len() != original_len {
        workspace.save_tools(&store)?;
    }
    remove_path_if_exists(&tool_dir)?;

    Ok(())
}

fn install_global_package(global_home: &GlobalHome, package: String) -> Result<()> {
    let install = ensure_global_package_installed(global_home, &package)?;
    let shim_path = write_global_shim(global_home, &package)?;

    println!("package: {package}");
    println!("scope: global");
    println!(
        "status: {}",
        match install.status {
            PackageInstallStatus::Installed => "installed",
            PackageInstallStatus::AlreadyInstalled => "already installed",
        }
    );
    println!("script: {}", install.entry_path.display());
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
    let tool_dir = global_home.tool_dir(&package);
    let shim_path = global_home.shim_path(&package);
    let had_tool_dir = tool_dir.exists();
    let had_shim = shim_path.exists();

    if store.packages.len() == original_len && !had_tool_dir && !had_shim {
        bail!("package `{package}` is not globally installed");
    }

    if store.packages.len() != original_len {
        global_home.save_tools(&store)?;
    }
    remove_path_if_exists(&tool_dir)?;
    remove_path_if_exists(&shim_path)?;

    println!("package: {package}");
    println!("scope: global");
    println!("status: uninstalled");
    println!("shim: {}", shim_path.display());

    Ok(())
}

fn ensure_workspace_package_installed(
    workspace: &Workspace,
    package: &str,
) -> Result<PackageInstallResult> {
    if !workspace.is_initialized() {
        workspace.init_with_options(InitOptions::default())?;
    }

    let mut store = workspace.load_tools()?;
    let entry_path = workspace.tool_entry_path(package);
    let already_on_disk = entry_path.is_file();

    if !already_on_disk {
        ensure_install_target_ready(&workspace.tool_dir(package), &entry_path, package)?;
        install_tool_from_hub(package, &workspace.tool_dir(package))?;
    }

    let manifest_updated = upsert_package_record(&mut store, package);
    if manifest_updated {
        workspace.save_tools(&store)?;
    }

    Ok(PackageInstallResult {
        entry_path,
        status: if already_on_disk && !manifest_updated {
            PackageInstallStatus::AlreadyInstalled
        } else {
            PackageInstallStatus::Installed
        },
    })
}

fn ensure_global_package_installed(
    global_home: &GlobalHome,
    package: &str,
) -> Result<PackageInstallResult> {
    global_home.init()?;

    let mut store = global_home.load_tools()?;
    let entry_path = global_home.tool_entry_path(package);
    let already_on_disk = entry_path.is_file();

    if !already_on_disk {
        ensure_install_target_ready(&global_home.tool_dir(package), &entry_path, package)?;
        install_tool_from_hub(package, &global_home.tool_dir(package))?;
    }

    let manifest_updated = upsert_package_record(&mut store, package);
    if manifest_updated {
        global_home.save_tools(&store)?;
    }

    Ok(PackageInstallResult {
        entry_path,
        status: if already_on_disk && !manifest_updated {
            PackageInstallStatus::AlreadyInstalled
        } else {
            PackageInstallStatus::Installed
        },
    })
}

fn ensure_install_target_ready(tool_dir: &Path, entry_path: &Path, package: &str) -> Result<()> {
    if !tool_dir.exists() {
        return Ok(());
    }

    if entry_path.is_file() {
        return Ok(());
    }

    bail!(
        "tool directory for `{package}` already exists at {}, but `{}` is missing",
        tool_dir.display(),
        entry_path.display()
    );
}

fn upsert_package_record(store: &mut crate::workspace::ToolStore, package: &str) -> bool {
    if package_exists(store, package) {
        return false;
    }

    store.packages.push(InstalledPackage {
        name: package.to_string(),
        version: None,
        path: None,
    });
    store
        .packages
        .sort_by(|left, right| left.name.cmp(&right.name));
    true
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove directory {}", path.display()))?;
    } else {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove file {}", path.display()))?;
    }

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

fn resolve_global_tool_target(global_home: &GlobalHome, target: &str) -> Option<PathBuf> {
    let entry = global_home.tool_entry_path(target);
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

    if let Some(script_path) = resolve_global_tool_target(global_home, target) {
        return build_tool_info("global-tool", script_path);
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
        env, fs,
        ffi::OsString,
        process::{self, Command},
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    use tokio::sync::Semaphore;

    use super::*;
    use crate::tool_hub::{OPENWALK_HUB_GIT_REF_ENV, OPENWALK_HUB_GIT_URL_ENV};

    static CWD_LOCK: Semaphore = Semaphore::const_new(1);
    static HUB_ENV_LOCK: Mutex<()> = Mutex::new(());

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

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                env::set_var(self.key, previous);
            } else {
                env::remove_var(self.key);
            }
        }
    }

    struct LocalHubRepo {
        _sandbox: TestDir,
        path: PathBuf,
    }

    impl LocalHubRepo {
        fn with_tool(name: &str, body: &str) -> Self {
            Self::with_tools(&[(name, body)])
        }

        fn with_tools(tools: &[(&str, &str)]) -> Self {
            let sandbox = TestDir::new();
            let path = sandbox.path.join("hub");
            for (name, body) in tools {
                fs::create_dir_all(path.join("tools").join(name))
                    .expect("hub tool directory should be created");
                fs::write(path.join("tools").join(name).join("main.scm"), body)
                    .expect("hub tool script should be written");
            }

            run_git(&path, &["init"]);
            run_git(&path, &["checkout", "-b", "main"]);
            run_git(&path, &["add", "."]);
            run_git(
                &path,
                &[
                    "-c",
                    "user.name=OpenWalk Tests",
                    "-c",
                    "user.email=tests@example.com",
                    "commit",
                    "-m",
                    "initial hub fixture",
                ],
            );

            Self {
                _sandbox: sandbox,
                path,
            }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    fn run_git(repo_dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_dir)
            .output()
            .expect("git command should launch in tests");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn install_test_hub_tool(name: &str, body: &str) -> (LocalHubRepo, EnvVarGuard, EnvVarGuard) {
        let repo = LocalHubRepo::with_tool(name, body);
        let url_guard =
            EnvVarGuard::set(OPENWALK_HUB_GIT_URL_ENV, repo.path().to_str().expect("utf8 path"));
        let ref_guard = EnvVarGuard::set(OPENWALK_HUB_GIT_REF_ENV, "main");
        (repo, url_guard, ref_guard)
    }

    fn install_test_hub_tools(
        tools: &[(&str, &str)],
    ) -> (LocalHubRepo, EnvVarGuard, EnvVarGuard) {
        let repo = LocalHubRepo::with_tools(tools);
        let url_guard =
            EnvVarGuard::set(OPENWALK_HUB_GIT_URL_ENV, repo.path().to_str().expect("utf8 path"));
        let ref_guard = EnvVarGuard::set(OPENWALK_HUB_GIT_REF_ENV, "main");
        (repo, url_guard, ref_guard)
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
        let _env_guard = HUB_ENV_LOCK.lock().expect("hub env lock should be acquired");
        let (_sandbox, workspace) = initialized_workspace();
        let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
            "browser-tools",
            r#"#| @meta
{
  "name": "browser-tools",
  "description": "Hub fixture workspace tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk exec browser-tools"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "workspace-ok")
"#,
        );

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
        assert!(workspace.tool_entry_path("browser-tools").exists());

        uninstall_package(&workspace, "browser-tools".to_string())
            .expect("uninstall should succeed");
        let after_uninstall = workspace
            .load_tools()
            .expect("tools should load after uninstall");
        assert!(after_uninstall.packages.is_empty());
        assert!(!workspace.tool_dir("browser-tools").exists());
    }

    #[test]
    fn install_workspace_tools_installs_declared_manifest_packages() {
        let _env_guard = HUB_ENV_LOCK.lock().expect("hub env lock should be acquired");
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        workspace
            .init_with_options(InitOptions {
                tools: vec!["browser-tools".to_string(), "v2ex-tools".to_string()],
                ..InitOptions::default()
            })
            .expect("workspace should initialize with declared tools");
        let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tools(&[
            (
                "browser-tools",
                r#"#| @meta
{
  "name": "browser-tools",
  "description": "Hub fixture workspace tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk install"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "workspace-ok")
"#,
            ),
            (
                "v2ex-tools",
                r#"#| @meta
{
  "name": "v2ex-tools",
  "description": "Second hub fixture tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk install"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "workspace-ok-2")
"#,
            ),
        ]);

        install_workspace_tools(&workspace, ProjectInstallArgs {})
            .expect("top-level install should install declared tools");

        assert!(workspace.tool_entry_path("browser-tools").exists());
        assert!(workspace.tool_entry_path("v2ex-tools").exists());
    }

    #[test]
    fn install_workspace_tools_without_manifest_fails() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());

        let error = install_workspace_tools(&workspace, ProjectInstallArgs {})
            .expect_err("install without manifest should fail");

        assert!(error.to_string().contains("openwalk init"));
    }

    #[test]
    fn handle_tool_add_and_remove_updates_store() {
        let _env_guard = HUB_ENV_LOCK.lock().expect("hub env lock should be acquired");
        let (_workspace_sandbox, workspace) = initialized_workspace();
        let (_global_sandbox, global_home) = initialized_global_home();
        let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
            "browser-tools",
            r#"#| @meta
{
  "name": "browser-tools",
  "description": "Hub fixture workspace tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk tool add browser-tools"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "workspace-ok")
"#,
        );

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
        assert!(workspace.tool_entry_path("browser-tools").exists());

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
        assert!(!workspace.tool_dir("browser-tools").exists());
    }

    #[test]
    fn handle_tool_install_and_uninstall_updates_global_store_and_shim() {
        let _env_guard = HUB_ENV_LOCK.lock().expect("hub env lock should be acquired");
        let (_workspace_sandbox, workspace) = initialized_workspace();
        let (_global_sandbox, global_home) = initialized_global_home();
        let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
            "browser-tools",
            r#"#| @meta
{
  "name": "browser-tools",
  "description": "Hub fixture global tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk tool install browser-tools"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "global-ok")
"#,
        );

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
        assert!(global_home.tool_entry_path("browser-tools").exists());
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
        assert!(!global_home.tool_dir("browser-tools").exists());
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
    fn load_tool_info_reads_global_script_metadata() {
        let _env_guard = HUB_ENV_LOCK.lock().expect("hub env lock should be acquired");
        let workspace_sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
        let (_global_sandbox, global_home) = initialized_global_home();
        let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
            "global-bing-search",
            r#"#| @meta
{
  "name": "global-bing-search",
  "description": "全局 Bing 搜索",
  "args": [],
  "returns": {
    "type": "object",
    "description": "{ results[] }"
  },
  "examples": ["openwalk exec global-bing-search"],
  "domains": ["www.bing.com"],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["search"]
}
|#
(define (main args) "ok")
"#,
        );

        install_global_package(&global_home, "global-bing-search".to_string())
            .expect("global install should succeed");

        let info = load_tool_info(&workspace, &global_home, "global-bing-search")
            .expect("global tool info should load");

        assert_eq!(info.name, "global-bing-search");
        assert_eq!(info.source, "global-tool");
        assert_eq!(info.metadata.description, "全局 Bing 搜索");
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
    async fn exec_tool_auto_installs_unknown_tools_from_hub() {
        let _env_guard = HUB_ENV_LOCK.lock().expect("hub env lock should be acquired");
        let workspace_sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
        let (_global_sandbox, global_home) = initialized_global_home();
        let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
            "remote.browser.open",
            r#"#| @meta
{
  "name": "remote.browser.open",
  "description": "Remote fixture tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk exec remote.browser.open"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "remote-ok")
"#,
        );

        exec_tool(
            &workspace,
            &global_home,
            ToolExecArgs {
                tool: "remote.browser.open".to_string(),
                args: vec!["https://example.com".to_string()],
            },
        )
        .await
        .expect("exec should install and run the hub tool");

        let installed = workspace
            .load_tools()
            .expect("tools should load after remote exec install");
        assert!(package_exists(&installed, "remote.browser.open"));
        assert!(workspace.tool_entry_path("remote.browser.open").exists());
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
        let _env_guard = HUB_ENV_LOCK.lock().expect("hub env lock should be acquired");
        let workspace_sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
        let (_global_sandbox, global_home) = initialized_global_home();
        let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
            "browser-tools",
            r#"#| @meta
{
  "name": "browser-tools",
  "description": "Global fixture tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk exec browser-tools"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "global-ok")
"#,
        );

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

        assert!(!workspace.is_initialized());
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
