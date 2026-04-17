use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const META_MARKER: &str = "@meta";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub args: Vec<ToolArgument>,
    pub returns: ToolReturn,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    pub read_only: bool,
    pub requires_login: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolArgument {
    pub name: String,
    #[serde(rename = "type")]
    pub arg_type: String,
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolReturn {
    #[serde(rename = "type")]
    pub return_type: String,
    pub description: String,
}

pub fn load_tool_metadata(script_path: &Path) -> Result<ToolMetadata> {
    let source = fs::read_to_string(script_path)
        .with_context(|| format!("failed to read script {}", script_path.display()))?;
    parse_tool_metadata(&source)?.ok_or_else(|| {
        anyhow::anyhow!(
            "tool script `{}` is missing a `#| @meta ... |#` header",
            script_path.display()
        )
    })
}

fn parse_tool_metadata(source: &str) -> Result<Option<ToolMetadata>> {
    let trimmed = source.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with("#|") {
        return Ok(None);
    }

    let Some(end) = trimmed.find("|#") else {
        return Ok(None);
    };

    let block = trimmed[2..end].trim();
    if !block.starts_with(META_MARKER) {
        return Ok(None);
    }

    let json = block[META_MARKER.len()..].trim();
    let metadata = serde_json::from_str(json).context("failed to parse tool metadata json")?;
    Ok(Some(metadata))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn parse_tool_metadata_reads_scheme_block_comment_header() {
        let source = r#"
            #| @meta
            {
              "name": "bing-search",
              "description": "Bing 搜索并返回结构化结果",
              "args": [
                {
                  "name": "query",
                  "type": "string",
                  "required": true,
                  "description": "搜索关键词"
                }
              ],
              "returns": {
                "type": "object",
                "description": "{ query, count, results[] }"
              },
              "examples": ["openwalk run bing-search -- \"Claude Code\" 10"],
              "domains": ["www.bing.com"],
              "readOnly": true,
              "requiresLogin": false,
              "tags": ["search", "bing"]
            }
            |#

            (define (main args) "ok")
        "#;

        let metadata = parse_tool_metadata(source)
            .expect("metadata parsing should succeed")
            .expect("metadata should exist");

        assert_eq!(metadata.name, "bing-search");
        assert_eq!(metadata.args.len(), 1);
        assert!(metadata.read_only);
        assert!(!metadata.requires_login);
    }

    #[test]
    fn parse_tool_metadata_returns_none_without_header() {
        let metadata = parse_tool_metadata("(define (main args) \"ok\")")
            .expect("metadata parsing should succeed");

        assert_eq!(metadata, None);
    }

    #[test]
    fn checked_in_scheme_examples_have_valid_metadata() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for relative_path in [
            ".openwalk/tools/bing-search/main.scm",
            ".openwalk/tools/v2ex-hot/main.scm",
        ] {
            let script_path = repo_root.join(relative_path);
            let metadata =
                load_tool_metadata(&script_path).expect("checked-in script metadata should parse");
            assert!(
                !metadata.name.is_empty(),
                "metadata name should not be empty for {}",
                script_path.display()
            );
        }
    }
}
