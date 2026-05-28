use std::path::PathBuf;

use serde::Serialize;

use crate::tool_metadata::{ToolArgument, ToolMetadata, ToolReturn};

#[derive(Debug, Clone, Serialize)]
// Compact response shape used by `tool list`.
pub(super) struct ToolListEntry {
    pub(super) name: String,
    pub(super) usage: String,
    pub(super) description: String,
    pub(super) source: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ToolInfoEntry {
    pub(super) name: String,
    pub(super) source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) script: Option<String>,
    pub(super) metadata: ToolMetadata,
}

#[derive(Debug, Serialize)]
pub(super) struct ToolInfoView {
    pub(super) name: String,
    pub(super) usage: String,
    pub(super) description: String,
    pub(super) source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) script: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) args: Vec<ToolArgument>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) options: Vec<ToolArgument>,
    pub(super) returns: ToolReturn,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) examples: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) domains: Vec<String>,
    #[serde(rename = "readOnly", skip_serializing_if = "is_false")]
    pub(super) read_only: bool,
    #[serde(rename = "requiresLogin", skip_serializing_if = "is_false")]
    pub(super) requires_login: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PackageInstallStatus {
    Installed,
    AlreadyInstalled,
}

#[derive(Debug, Clone)]
pub(super) struct PackageInstallResult {
    pub(super) entry_path: PathBuf,
    pub(super) status: PackageInstallStatus,
}

fn is_false(value: &bool) -> bool {
    !*value
}
