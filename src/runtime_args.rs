use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::output::{parse_output_format, OutputFormat};

const OPENWALK_SESSION_NAME_ENV: &str = "OPENWALK_SESSION_NAME";

#[derive(Debug, Clone)]
pub struct RuntimeInvocationArgs {
    pub runtime_args: Vec<String>,
    pub session: Option<String>,
    pub all: bool,
    pub output_format: OutputFormat,
}

#[derive(Debug, Clone)]
pub struct BrowserLaunchOptions {
    pub session: Option<String>,
    pub headless: Option<bool>,
    pub profile: Option<PathBuf>,
    pub create_session_if_missing: bool,
}

#[derive(Debug, Clone)]
pub struct BrowserOpenArgs {
    pub url: String,
    pub session: Option<String>,
    pub headless: Option<bool>,
    pub profile: Option<PathBuf>,
    pub create_session_if_missing: bool,
    pub new_tab: bool,
}

impl BrowserLaunchOptions {
    pub fn with_session(session: Option<String>) -> Self {
        Self {
            session,
            ..Self::default()
        }
    }
}

impl Default for BrowserLaunchOptions {
    fn default() -> Self {
        Self {
            session: None,
            headless: None,
            profile: None,
            create_session_if_missing: true,
        }
    }
}

pub fn extract_common_runtime_args(args: &[String]) -> Result<RuntimeInvocationArgs> {
    let mut runtime_args = Vec::new();
    let mut session = std::env::var(OPENWALK_SESSION_NAME_ENV)
        .ok()
        .filter(|value| !value.is_empty());
    let mut all: bool = false;
    let mut output_format = OutputFormat::default();
    let mut passthrough = false;
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if passthrough {
            runtime_args.push(arg.clone());
            index += 1;
            continue;
        }

        if arg == "--" {
            passthrough = true;
            index += 1;
            continue;
        }

        if arg == "-a" || arg == "--all" {
            all = true;
            index += 1;
            continue;
        }

        if arg == "-s" || arg == "--session" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow::anyhow!("expects a session name after `{arg}`"))?;
            if value.is_empty() {
                bail!("received an empty session name");
            }
            session = Some(value.clone());
            index += 2;
            continue;
        }

        if let Some(value) = arg.strip_prefix("-s=") {
            if value.is_empty() {
                bail!("received an empty session name");
            }
            session = Some(value.to_string());
            index += 1;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--session=") {
            if value.is_empty() {
                bail!("received an empty session name");
            }
            session = Some(value.to_string());
            index += 1;
            continue;
        }

        if arg == "-f" || arg == "--format" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow::anyhow!("expects a format name after `{arg}`"))?;
            output_format = parse_output_format(value.as_str())?;
            index += 2;
            continue;
        }

        if let Some(value) = arg.strip_prefix("-f=") {
            output_format = parse_output_format(value)?;
            index += 1;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--format=") {
            output_format = parse_output_format(value)?;
            index += 1;
            continue;
        }

        runtime_args.push(arg.clone());
        index += 1;
    }

    Ok(RuntimeInvocationArgs {
        runtime_args,
        session,
        all,
        output_format,
    })
}

pub fn parse_browser_open_runtime_args(
    args: &[String],
    session: Option<String>,
) -> Result<BrowserOpenArgs> {
    let mut headless = None;
    let mut profile = None;
    let mut new_tab = false;
    let mut positional = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if arg == "--headed" {
            headless = Some(false);
            index += 1;
            continue;
        }
        if arg == "--new-tab" {
            new_tab = true;
            index += 1;
            continue;
        }
        if arg == "--profile" {
            let value = args.get(index + 1).ok_or_else(|| {
                anyhow::anyhow!("`browser-open` expects a profile path after `--profile`")
            })?;
            if value.is_empty() {
                bail!("`browser-open` received an empty profile path");
            }
            profile = Some(PathBuf::from(value));
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--profile=") {
            if value.is_empty() {
                bail!("`browser-open` received an empty profile path");
            }
            profile = Some(PathBuf::from(value));
            index += 1;
            continue;
        }
        if arg == "--" {
            positional.extend(args.iter().skip(index + 1).cloned());
            break;
        }
        if arg.starts_with('-') {
            bail!(
                "`browser-open` does not support option `{arg}`. Supported options are `--headed`, `--profile`, and `--new-tab`"
            );
        }
        positional.push(arg.clone());
        index += 1;
    }

    if positional.len() != 1 {
        bail!("`browser-open` expects exactly one url argument");
    }

    Ok(BrowserOpenArgs {
        url: positional.remove(0),
        session,
        headless,
        profile,
        create_session_if_missing: true,
        new_tab,
    })
}

pub fn parse_browser_close_runtime_args(args: &[String]) -> Result<()> {
    if args.is_empty() {
        Ok(())
    } else {
        bail!("`browser-close` does not accept positional arguments")
    }
}

#[cfg(test)]
mod tests {
    use std::{env, ffi::OsString, path::PathBuf, sync::Mutex};

    use crate::output::OutputFormat;

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                env::set_var(self.key, previous);
            } else {
                env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn extract_common_runtime_args_parses_session_and_escape() {
        let parsed = extract_common_runtime_args(&[
            "https://example.com".to_string(),
            "-s=qa".to_string(),
            "--".to_string(),
            "--session=raw".to_string(),
        ])
        .expect("runtime args should parse");

        assert_eq!(parsed.session.as_deref(), Some("qa"));
        assert_eq!(parsed.output_format, OutputFormat::Yaml);
        assert_eq!(
            parsed.runtime_args,
            vec![
                "https://example.com".to_string(),
                "--session=raw".to_string()
            ]
        );
    }

    #[test]
    fn extract_common_runtime_args_parses_short_format_flag() {
        let parsed =
            extract_common_runtime_args(&["-f=md".to_string(), "https://example.com".to_string()])
                .expect("runtime args should parse with format");

        assert_eq!(parsed.output_format, OutputFormat::Md);
        assert_eq!(parsed.runtime_args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn extract_common_runtime_args_parses_json_format_flag() {
        let parsed = extract_common_runtime_args(&[
            "--format=json".to_string(),
            "https://example.com".to_string(),
        ])
        .expect("runtime args should parse with json format");

        assert_eq!(parsed.output_format, OutputFormat::Json);
        assert_eq!(parsed.runtime_args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn extract_common_runtime_args_uses_session_from_env_by_default() {
        let _env_guard = ENV_LOCK
            .lock()
            .expect("session env lock should be acquired");
        let _session_guard = EnvVarGuard::set(OPENWALK_SESSION_NAME_ENV, "env-default");

        let parsed = extract_common_runtime_args(&["https://example.com".to_string()])
            .expect("runtime args should pick up session from env");

        assert_eq!(parsed.session.as_deref(), Some("env-default"));
        assert_eq!(parsed.runtime_args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn extract_common_runtime_args_cli_session_overrides_env() {
        let _env_guard = ENV_LOCK
            .lock()
            .expect("session env lock should be acquired");
        let _session_guard = EnvVarGuard::set(OPENWALK_SESSION_NAME_ENV, "env-default");

        let parsed = extract_common_runtime_args(&[
            "https://example.com".to_string(),
            "--session=cli-session".to_string(),
        ])
        .expect("runtime args should prefer explicit cli session");

        assert_eq!(parsed.session.as_deref(), Some("cli-session"));
        assert_eq!(parsed.runtime_args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn extract_common_runtime_args_ignores_empty_session_env() {
        let _env_guard = ENV_LOCK
            .lock()
            .expect("session env lock should be acquired");
        let _session_guard = EnvVarGuard::set(OPENWALK_SESSION_NAME_ENV, "");

        let parsed = extract_common_runtime_args(&["https://example.com".to_string()])
            .expect("empty session env should be treated as unset");

        assert_eq!(parsed.session, None);
        assert_eq!(parsed.runtime_args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn extract_common_runtime_args_preserves_browser_specific_flags() {
        let parsed = extract_common_runtime_args(&[
            "--new-tab".to_string(),
            "https://example.com".to_string(),
        ])
        .expect("runtime args should preserve browser-only flags");

        assert_eq!(
            parsed.runtime_args,
            vec!["--new-tab".to_string(), "https://example.com".to_string()]
        );
    }

    #[test]
    fn extract_common_runtime_args_rejects_unknown_format() {
        let error = extract_common_runtime_args(&["--format=toml".to_string()])
            .expect_err("unknown format should fail");
        assert!(error.to_string().contains("unsupported output format"));
    }

    #[test]
    fn parse_browser_open_runtime_args_reads_browser_flags() {
        let parsed = parse_browser_open_runtime_args(
            &[
                "https://example.com".to_string(),
                "--headed".to_string(),
                "--new-tab".to_string(),
                "--profile=/tmp/openwalk-profile".to_string(),
            ],
            Some("qa".to_string()),
        )
        .expect("browser-open args should parse");

        assert_eq!(parsed.url, "https://example.com");
        assert_eq!(parsed.session.as_deref(), Some("qa"));
        assert_eq!(parsed.headless, Some(false));
        assert!(parsed.create_session_if_missing);
        assert!(parsed.new_tab);
        assert_eq!(parsed.profile, Some(PathBuf::from("/tmp/openwalk-profile")));
    }

    #[test]
    fn parse_browser_close_runtime_args_rejects_positional_args() {
        let err = parse_browser_close_runtime_args(&["extra".to_string()])
            .expect_err("browser-close should reject positional args");

        assert!(err
            .to_string()
            .contains("does not accept positional arguments"));
    }
}
