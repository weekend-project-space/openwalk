use std::{fs, path::PathBuf};

use anyhow::{Context, Result};

use crate::workspace::GlobalHome;

pub(super) fn write_global_shim(global_home: &GlobalHome, package: &str) -> Result<PathBuf> {
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
        use std::os::unix::fs::PermissionsExt;

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
