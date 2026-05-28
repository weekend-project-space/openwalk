mod exec;
mod info;
mod init;
mod install;
mod list;
mod presentation;
mod shim;
mod target;
mod types;

use anyhow::Result;

use crate::{
    cli::{Cli, Command, ToolCommand},
    workspace::{GlobalHome, Workspace},
};

use exec::{exec_tool, run_local};
use info::show_tool_info;
use init::init_workspace;
use install::{
    install_global_package, install_package, install_workspace_tools, uninstall_global_package,
    uninstall_package,
};
use list::list_tools;

#[cfg(test)]
use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::{
    builtin_tools,
    cli::{ProjectInstallArgs, ToolExecArgs},
    tool_metadata::{ToolArgument, ToolMetadata, ToolReturn},
    workspace::{InitOptions, InstalledPackage},
};

#[cfg(test)]
use info::{build_builtin_tool_info, build_tool_info_view, load_tool_info};
#[cfg(test)]
use list::{render_tool_list_lines, workspace_tool_entries};
#[cfg(test)]
use presentation::tool_usage_from_metadata;
#[cfg(test)]
use target::{package_exists, resolve_run_target, resolve_script_target};
#[cfg(test)]
use types::{ToolInfoEntry, ToolListEntry};

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
        ToolCommand::List { format } => list_tools(workspace, global_home, format),
        ToolCommand::Info { tool, format } => show_tool_info(workspace, global_home, tool, format),
    }
}

#[cfg(test)]
mod tests;
