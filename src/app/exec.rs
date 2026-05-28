use std::path::Path;

use anyhow::{bail, Result};
use serde_json::json;

use crate::{
    browser::{
        attach_browser_session_with_options, ensure_browser_session_with_options, BrowserService,
        BrowserSessionLaunchOptions, EphemeralLaunchOptions,
    },
    cli::ToolExecArgs,
    output::print_execution_result,
    runtime_args::{
        extract_common_runtime_args, parse_browser_close_runtime_args,
        parse_browser_open_runtime_args, BrowserLaunchOptions,
    },
    scheme_runtime, value_codec,
    workspace::{GlobalHome, Workspace},
};

use super::{
    install::ensure_workspace_package_installed,
    target::{
        resolve_global_tool_target, resolve_run_target, resolve_script_target,
        resolve_workspace_tool_target, tool_exists,
    },
};

pub(super) async fn run_local(args: ToolExecArgs) -> Result<()> {
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

pub(super) async fn exec_tool(
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

    if let Some(script_path) = resolve_workspace_tool_target(workspace, &tool)? {
        return run_scheme_script(global_home, "exec", &script_path, &cli_args).await;
    }

    if tool_exists(&tool) {
        return run_builtin_tool(global_home, "exec", &tool, &cli_args).await;
    }

    if let Some(script_path) = resolve_global_tool_target(global_home, &tool)? {
        return run_scheme_script(global_home, "exec", &script_path, &cli_args).await;
    }

    let installed = ensure_workspace_package_installed(workspace, &tool)?;
    run_scheme_script(global_home, "exec", &installed.entry_path, &cli_args).await
}

async fn run_scheme_script(
    global_home: &GlobalHome,
    _mode: &str,
    script_path: &Path,
    args: &[String],
) -> Result<()> {
    let parsed_args = extract_common_runtime_args(args)?;
    let launch_options = BrowserLaunchOptions::with_session(parsed_args.session.clone());
    let browser = create_browser_service(global_home, launch_options).await?;
    let result = scheme_runtime::execute_script(
        script_path,
        &parsed_args.runtime_args,
        browser.client(),
        parsed_args.session.clone(),
    )
    .await;

    let output_payload = if parsed_args.all {
        result.map(|result| {
            json!({
                // "mode": mode,
                "script": script_path.display().to_string(),
                "args": parsed_args.runtime_args,
                "result": value_codec::scheme_value_to_json(&result),
                // "status": "executed",
            })
        })
    } else {
        result.map(|result| value_codec::scheme_value_to_json(&result))
    };

    let shutdown = browser.shutdown().await;

    let payload = output_payload?;
    shutdown?;
    print_execution_result(parsed_args.output_format, &payload)?;

    Ok(())
}

async fn run_builtin_tool(
    global_home: &GlobalHome,
    _mode: &str,
    tool: &str,
    args: &[String],
) -> Result<()> {
    let parsed_args = extract_common_runtime_args(args)?;
    let mut launch_options = BrowserLaunchOptions::with_session(parsed_args.session.clone());
    let runtime_args = if tool == "browser-open" {
        let open_args = parse_browser_open_runtime_args(
            &parsed_args.runtime_args,
            parsed_args.session.clone(),
        )?;
        launch_options.session = open_args.session.clone();
        if open_args.headless.is_some() {
            launch_options.headless = open_args.headless;
        }
        if open_args.profile.is_some() {
            launch_options.profile = open_args.profile;
        }
        launch_options.create_session_if_missing = open_args.create_session_if_missing;
        let mut args = vec![open_args.url];
        if open_args.new_tab {
            args.push("true".to_string());
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
    let result = scheme_runtime::execute_builtin(
        tool,
        &runtime_args,
        browser.client(),
        parsed_args.session.clone(),
    )
    .await;

    let output_payload = if parsed_args.all {
        result.map(|result| {
            json!({
                // "mode": mode,
                "tool": tool,
                "args": parsed_args.runtime_args,
                "result": value_codec::scheme_value_to_json(&result),
                // "status": "executed",
            })
        })
    } else {
        result.map(|result| value_codec::scheme_value_to_json(&result))
    };

    let shutdown = browser.shutdown().await;

    let payload = output_payload?;
    shutdown?;
    print_execution_result(parsed_args.output_format, &payload)?;

    Ok(())
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
