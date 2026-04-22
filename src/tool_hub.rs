use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};

use crate::tool_ref::{tool_ref_relative_path, validate_tool_ref};

pub const OPENWALK_HUB_GIT_URL_ENV: &str = "OPENWALK_HUB_GIT_URL";
pub const OPENWALK_HUB_GIT_REF_ENV: &str = "OPENWALK_HUB_GIT_REF";
const DEFAULT_OPENWALK_HUB_GIT_URL: &str = "https://github.com/weekend-project-space/openwalkhub";
const DEFAULT_OPENWALK_HUB_GIT_REF: &str = "main";
const TOOL_ENTRY_FILE: &str = "main.scm";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHubConfig {
    pub git_url: String,
    pub git_ref: String,
}

impl ToolHubConfig {
    pub fn from_env() -> Self {
        Self {
            git_url: env::var(OPENWALK_HUB_GIT_URL_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| DEFAULT_OPENWALK_HUB_GIT_URL.to_string()),
            git_ref: env::var(OPENWALK_HUB_GIT_REF_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| DEFAULT_OPENWALK_HUB_GIT_REF.to_string()),
        }
    }
}

pub fn install_tool_from_hub(package: &str, destination_dir: &Path) -> Result<PathBuf> {
    validate_tool_ref(package)?;

    if destination_dir.exists() {
        bail!(
            "refusing to install `{package}` into existing path {}",
            destination_dir.display()
        );
    }

    let config = ToolHubConfig::from_env();
    let checkout_root = TempCheckoutDir::new()?;
    let checkout_dir = checkout_root.path().join("hub");

    clone_hub_repo(&config, &checkout_dir)?;

    let source_dir = checkout_dir
        .join("tools")
        .join(tool_ref_relative_path(package)?);
    let source_entry = source_dir.join(TOOL_ENTRY_FILE);
    if !source_dir.is_dir() {
        bail!(
            "tool `{package}` was not found under tools/{package} in {}",
            config.git_url
        );
    }
    if !source_entry.is_file() {
        bail!(
            "tool `{package}` is missing `{}` in {}",
            TOOL_ENTRY_FILE,
            source_dir.display()
        );
    }

    copy_directory_recursively(&source_dir, destination_dir)?;

    let installed_entry = destination_dir.join(TOOL_ENTRY_FILE);
    if !installed_entry.is_file() {
        bail!(
            "tool `{package}` was copied from {} but `{}` is still missing in {}",
            config.git_url,
            TOOL_ENTRY_FILE,
            destination_dir.display()
        );
    }

    Ok(installed_entry)
}

fn clone_hub_repo(config: &ToolHubConfig, checkout_dir: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--branch")
        .arg(&config.git_ref)
        .arg(&config.git_url)
        .arg(checkout_dir)
        .output()
        .context("failed to launch `git clone` for the openwalk tool hub")?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        bail!(
            "failed to clone openwalk tool hub {} (ref {})",
            config.git_url,
            config.git_ref
        );
    }

    bail!(
        "failed to clone openwalk tool hub {} (ref {}): {}",
        config.git_url,
        config.git_ref,
        detail
    );
}

fn copy_directory_recursively(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;

    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to inspect entry under {}", source.display()))?;
        let entry_type = entry.file_type().with_context(|| {
            format!(
                "failed to determine file type for {}",
                entry.path().display()
            )
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if entry_type.is_dir() {
            copy_directory_recursively(&source_path, &destination_path)?;
            continue;
        }

        if entry_type.is_file() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
            continue;
        }

        bail!(
            "unsupported filesystem entry while copying tool package: {}",
            source_path.display()
        );
    }

    Ok(())
}

struct TempCheckoutDir {
    path: PathBuf,
}

impl TempCheckoutDir {
    fn new() -> Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock appears to be before unix epoch")?
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "openwalk-hub-checkout-{}-{timestamp}",
            process::id()
        ));
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCheckoutDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
