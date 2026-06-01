use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::{
    builtin_tools,
    tool_ref::{is_explicit_script_target, script_target_path, validate_tool_ref},
    workspace::{GlobalHome, ToolStore, Workspace},
};

pub(super) fn tool_exists(tool: &str) -> bool {
    builtin_tools::SCHEME_BUILTINS.contains(&tool)
}

pub(super) fn package_exists(store: &ToolStore, package: &str) -> bool {
    store.packages.iter().any(|item| item.name == package)
}

pub(super) const KIT_TOOL_NAMESPACES: &[&str] = &["sys", "debug"];

pub(super) fn resolve_workspace_tool_target(
    workspace: &Workspace,
    target: &str,
) -> Result<Option<PathBuf>> {
    validate_tool_ref(target)?;
    let entry = workspace.tool_entry_path(target);
    if entry.exists() {
        Ok(Some(entry))
    } else {
        Ok(None)
    }
}

pub(super) fn resolve_global_tool_target(
    global_home: &GlobalHome,
    target: &str,
) -> Result<Option<PathBuf>> {
    validate_tool_ref(target)?;
    let entry = global_home.tool_entry_path(target);
    if entry.exists() {
        Ok(Some(entry))
    } else {
        Ok(None)
    }
}

pub(super) fn resolve_kit_tool_target(
    global_home: &GlobalHome,
    target: &str,
) -> Result<Option<PathBuf>> {
    validate_tool_ref(target)?;

    for candidate in kit_lookup_candidates(target) {
        let entry = global_home.kit_tool_entry_path(candidate.as_str());
        if entry.exists() {
            return Ok(Some(entry));
        }
    }

    Ok(None)
}

fn kit_lookup_candidates(target: &str) -> Vec<String> {
    if KIT_TOOL_NAMESPACES
        .iter()
        .any(|namespace| target.starts_with(&format!("{namespace}/")))
    {
        return vec![target.to_string()];
    }

    if !target.contains('/') {
        return vec![format!("sys/{target}")];
    }

    Vec::new()
}

pub(super) fn resolve_script_target(target: &str) -> Result<Option<PathBuf>> {
    if !is_explicit_script_target(target) {
        return Ok(None);
    }

    let candidate = PathBuf::from(script_target_path(target));

    if candidate.exists() {
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        bail!("script path `{target}` exists but is not a file");
    }

    bail!("scheme script `{target}` was not found");
}

pub(super) fn resolve_run_target(workspace: &Workspace, target: &str) -> Result<PathBuf> {
    if let Some(path) = resolve_script_target(target)? {
        return Ok(path);
    }

    validate_tool_ref(target)?;

    let entry = workspace.tool_entry_path(target);
    if entry.exists() {
        return Ok(entry);
    }

    bail!(
        "tool `{target}` was not found. Expected `{}`",
        entry.display()
    )
}
