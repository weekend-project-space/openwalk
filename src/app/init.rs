use anyhow::Result;
use serde_json::json;

use crate::{
    cli::InitArgs,
    output::{parse_output_format, print_execution_result},
    workspace::{InitOptions, Workspace},
};

pub(super) fn init_workspace(workspace: &Workspace, args: InitArgs) -> Result<()> {
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
