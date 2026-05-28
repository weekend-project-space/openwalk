use std::{fs, path::Path};

use anyhow::{bail, Context, Result};

use crate::{
    cli::ProjectInstallArgs,
    tool_hub::install_tool_from_hub,
    tool_ref::validate_tool_ref,
    workspace::{GlobalHome, InitOptions, InstalledPackage, ToolStore, Workspace},
};

use super::{
    shim::write_global_shim,
    target::package_exists,
    types::{PackageInstallResult, PackageInstallStatus},
};

pub(super) fn install_workspace_tools(
    workspace: &Workspace,
    args: ProjectInstallArgs,
) -> Result<()> {
    let _ = args;
    install_declared_workspace_tools(workspace)?;
    Ok(())
}

pub(super) fn install_package(workspace: &Workspace, package: String) -> Result<()> {
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

pub(super) fn uninstall_package(workspace: &Workspace, package: String) -> Result<()> {
    remove_workspace_package(workspace, &package)?;

    println!("package: {package}");
    println!("status: uninstalled");
    println!("scope: workspace");

    Ok(())
}

pub(super) fn install_named_workspace_tools(
    workspace: &Workspace,
    packages: &[String],
) -> Result<()> {
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
    validate_tool_ref(package)?;
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

pub(super) fn install_global_package(global_home: &GlobalHome, package: String) -> Result<()> {
    let install = ensure_global_package_installed(global_home, &package)?;
    let shim_path = write_global_shim(global_home, &package)?;
    let shim_name = shim_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(package.as_str());

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
        "hint: add {} to PATH to run `{shim_name}` directly",
        global_home.bin_dir().display()
    );

    Ok(())
}

pub(super) fn uninstall_global_package(global_home: &GlobalHome, package: String) -> Result<()> {
    validate_tool_ref(&package)?;
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

pub(super) fn ensure_workspace_package_installed(
    workspace: &Workspace,
    package: &str,
) -> Result<PackageInstallResult> {
    validate_tool_ref(package)?;
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
    validate_tool_ref(package)?;
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

fn upsert_package_record(store: &mut ToolStore, package: &str) -> bool {
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
