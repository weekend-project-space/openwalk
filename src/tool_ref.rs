use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

pub fn validate_tool_ref(tool_ref: &str) -> Result<()> {
    let trimmed = tool_ref.trim();
    if trimmed.is_empty() {
        bail!("tool ref must not be empty");
    }

    for segment in trimmed.split('/') {
        if segment.is_empty() {
            bail!("tool ref `{tool_ref}` contains an empty path segment");
        }
        if segment == "." || segment == ".." {
            bail!("tool ref `{tool_ref}` must not contain `.` or `..` path segments");
        }
        if segment.contains('\\') {
            bail!("tool ref `{tool_ref}` must use `/` as the namespace separator");
        }
        if segment.contains(':') {
            bail!("tool ref `{tool_ref}` must not contain `:`");
        }
    }

    Ok(())
}

pub fn is_explicit_script_target(target: &str) -> bool {
    if target.starts_with("file:") {
        return true;
    }

    let path = Path::new(target);
    path.is_absolute()
        || target.ends_with(".scm")
        || target.starts_with("./")
        || target.starts_with("../")
        || target.starts_with(".\\")
        || target.starts_with("..\\")
}

pub fn script_target_path(target: &str) -> &str {
    target.strip_prefix("file:").unwrap_or(target)
}

pub fn tool_ref_shim_name(tool_ref: &str) -> String {
    tool_ref
        .chars()
        .map(|ch| if ch == '/' || ch == '\\' { '-' } else { ch })
        .collect()
}

pub fn relative_tool_ref_from_tool_dir(tools_dir: &Path, entry_path: &Path) -> Option<String> {
    let tool_dir = entry_path.parent()?;
    let relative = tool_dir.strip_prefix(tools_dir).ok()?;
    let mut segments = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(segment) = component else {
            return None;
        };
        let segment = segment.to_str()?;
        if segment.is_empty() {
            return None;
        }
        segments.push(segment);
    }

    if segments.is_empty() {
        return None;
    }

    Some(segments.join("/"))
}

pub fn tool_ref_relative_path(tool_ref: &str) -> Result<PathBuf> {
    validate_tool_ref(tool_ref)?;

    let mut path = PathBuf::new();
    for segment in tool_ref.split('/') {
        path.push(segment);
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_tool_ref_accepts_flat_and_namespaced_refs() {
        validate_tool_ref("bing-search").expect("flat ref should be valid");
        validate_tool_ref("v2ex/hot").expect("namespaced ref should be valid");
        validate_tool_ref("remote.browser.open").expect("dots inside a segment should be valid");
    }

    #[test]
    fn validate_tool_ref_rejects_invalid_path_segments() {
        let invalid = ["", "v2ex//hot", "./hot", "../hot", "v2ex/../hot"];

        for value in invalid {
            let err = validate_tool_ref(value).expect_err("ref should be rejected");
            assert!(!err.to_string().is_empty());
        }
    }

    #[test]
    fn validate_tool_ref_rejects_reserved_characters() {
        let err = validate_tool_ref("v2ex\\hot").expect_err("backslash should be rejected");
        assert!(err.to_string().contains("namespace separator"));

        let err = validate_tool_ref("hub:v2ex/hot").expect_err("colon should be rejected");
        assert!(err.to_string().contains("must not contain `:`"));
    }

    #[test]
    fn explicit_script_targets_require_path_like_syntax() {
        assert!(is_explicit_script_target("./demo.scm"));
        assert!(is_explicit_script_target("../demo"));
        assert!(is_explicit_script_target("/tmp/demo.scm"));
        assert!(is_explicit_script_target("file:./demo"));
        assert!(is_explicit_script_target("v2ex/hot.scm"));
        assert!(!is_explicit_script_target("v2ex/hot"));
    }

    #[test]
    fn relative_tool_ref_from_tool_dir_formats_nested_paths() {
        let tools_dir = Path::new("/tmp/work/.openwalk/tools");
        let entry = Path::new("/tmp/work/.openwalk/tools/v2ex/hot/main.scm");

        let tool_ref = relative_tool_ref_from_tool_dir(tools_dir, entry)
            .expect("nested tool ref should resolve");

        assert_eq!(tool_ref, "v2ex/hot");
    }

    #[test]
    fn tool_ref_shim_name_replaces_slashes() {
        assert_eq!(tool_ref_shim_name("v2ex/hot"), "v2ex-hot");
    }
}
