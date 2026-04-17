use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(not(unix))]
use std::process::Stdio;

use anyhow::{anyhow, bail, Context, Result};
use chromiumoxide::{detection::DetectionOptions, handler::HandlerConfig, Browser, Handler};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::workspace::GlobalHome;

use super::{
    actor::BrowserActor,
    types::BrowserValue,
    util::{browser_request_timeout, env_flag_is_truthy, session_connect_timeout},
};

const SESSION_FILE: &str = "session.json";
const SESSION_LOG_FILE: &str = "browser.log";
const SESSION_CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionState {
    pub session: String,
    pub pid: u32,
    pub port: u16,
    pub http_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ws_url: Option<String>,
    pub profile_dir: String,
    #[serde(default = "default_headless_mode")]
    pub headless: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_target_id: Option<String>,
    pub started_at: u64,
}

#[derive(Debug, Clone)]
pub struct BrowserSessionHandle {
    manifest_path: PathBuf,
    state: BrowserSessionState,
}

#[derive(Debug, Clone, Default)]
pub struct BrowserSessionLaunchOptions {
    pub requested_headless: Option<bool>,
    pub requested_profile_dir: Option<PathBuf>,
}

impl BrowserSessionHandle {
    pub fn state(&self) -> &BrowserSessionState {
        &self.state
    }

    pub fn active_target_id(&self) -> Option<&str> {
        self.state.active_target_id.as_deref()
    }

    pub fn http_url(&self) -> &str {
        self.state.http_url.as_str()
    }

    pub fn set_active_target_id(&mut self, active_target_id: Option<String>) -> Result<()> {
        if self.state.active_target_id == active_target_id {
            return Ok(());
        }

        self.state.active_target_id = active_target_id;
        save_state(&self.manifest_path, &self.state)
    }

    pub fn remove_manifest(&self) -> Result<()> {
        remove_state(&self.manifest_path)
    }
}

pub async fn ensure_browser_session_with_options(
    global_home: &GlobalHome,
    session_name: &str,
    options: BrowserSessionLaunchOptions,
) -> Result<BrowserSessionHandle> {
    start_browser_session(global_home, session_name, options).await
}

pub async fn attach_browser_session_with_options(
    global_home: &GlobalHome,
    session_name: &str,
    options: BrowserSessionLaunchOptions,
) -> Result<BrowserSessionHandle> {
    let BrowserSessionLaunchOptions {
        requested_headless,
        requested_profile_dir,
    } = options;

    validate_session_name(session_name)?;
    global_home.init()?;

    let requested_profile_dir = requested_profile_dir
        .as_deref()
        .map(normalize_profile_dir)
        .transpose()?;

    let mut handle = load_state(global_home, session_name)?
        .ok_or_else(|| anyhow!("browser session `{session_name}` is not found"))?;

    if let Some((browser, handler)) = try_connect_session(handle.http_url()).await? {
        if let Some(requested_headless) = requested_headless {
            ensure_session_headless_matches(&handle, requested_headless)?;
        }
        if let Some(requested_profile_dir) = requested_profile_dir.as_deref() {
            ensure_session_profile_matches(&handle, requested_profile_dir)?;
        }
        let details = fetch_session_details(browser, handler).await?;
        handle.state.ws_url = details.ws_url;
        handle.state.active_target_id = resolve_active_target_id(
            handle.state.active_target_id.as_deref(),
            &details.page_target_ids,
        );
        save_state(&handle.manifest_path, &handle.state)?;
        return Ok(handle);
    }

    remove_state(&handle.manifest_path)?;
    bail!("browser session `{session_name}` is not running")
}

pub async fn start_browser_session(
    global_home: &GlobalHome,
    session_name: &str,
    options: BrowserSessionLaunchOptions,
) -> Result<BrowserSessionHandle> {
    let BrowserSessionLaunchOptions {
        requested_headless,
        requested_profile_dir,
    } = options;

    validate_session_name(session_name)?;
    global_home.init()?;

    let profile_dir =
        resolve_session_profile_dir(global_home, session_name, requested_profile_dir.as_deref())?;

    if let Some(mut handle) = load_state(global_home, session_name)? {
        if let Some((browser, handler)) = try_connect_session(handle.http_url()).await? {
            if let Some(requested_headless) = requested_headless {
                ensure_session_headless_matches(&handle, requested_headless)?;
            }
            ensure_session_profile_matches(&handle, &profile_dir)?;
            let details = fetch_session_details(browser, handler).await?;
            handle.state.ws_url = details.ws_url;
            handle.state.active_target_id = resolve_active_target_id(
                handle.state.active_target_id.as_deref(),
                &details.page_target_ids,
            );
            save_state(&handle.manifest_path, &handle.state)?;
            return Ok(handle);
        }

        remove_state(&handle.manifest_path)?;
    }

    let session_dir = global_home.browser_session_dir(session_name);
    fs::create_dir_all(&session_dir).with_context(|| {
        format!(
            "failed to create session directory {}",
            session_dir.display()
        )
    })?;

    fs::create_dir_all(&profile_dir).with_context(|| {
        format!(
            "failed to create profile directory {}",
            profile_dir.display()
        )
    })?;
    cleanup_stale_profile_singleton(&profile_dir)?;

    let log_path = session_dir.join(SESSION_LOG_FILE);
    let port = pick_free_port()?;
    let http_url = session_http_url(port);
    let headless = requested_headless.unwrap_or_else(default_headless_mode);
    let pid = launch_browser_process(port, &profile_dir, &log_path, headless)?;

    let (browser, handler) =
        wait_for_session_connect(http_url.as_str(), &log_path, &profile_dir).await?;
    let details = fetch_session_details(browser, handler).await?;
    let state = BrowserSessionState {
        session: session_name.to_string(),
        pid,
        port,
        http_url,
        ws_url: details.ws_url,
        profile_dir: profile_dir.display().to_string(),
        headless,
        active_target_id: resolve_active_target_id(None, &details.page_target_ids),
        started_at: unix_timestamp_now()?,
    };

    let manifest_path = session_manifest_path(global_home, session_name);
    save_state(&manifest_path, &state)?;
    Ok(BrowserSessionHandle {
        manifest_path,
        state,
    })
}

pub async fn connect_to_session(state: &BrowserSessionState) -> Result<(Browser, Handler)> {
    tokio::time::timeout(
        session_connect_timeout(),
        Browser::connect_with_config(state.http_url.clone(), browser_handler_config()),
    )
    .await
    .with_context(|| format!("timed out attaching to browser session `{}`", state.session))?
    .with_context(|| format!("failed to attach to browser session `{}`", state.session))
}

fn validate_session_name(session_name: &str) -> Result<()> {
    let valid = !session_name.is_empty()
        && session_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        bail!("invalid session name `{session_name}`. Use only letters, numbers, `.`, `_`, and `-`")
    }
}

fn default_headless_mode() -> bool {
    true
}

fn ensure_session_headless_matches(
    handle: &BrowserSessionHandle,
    requested_headless: bool,
) -> Result<()> {
    if handle.state.headless == requested_headless {
        return Ok(());
    }

    let requested_mode = if requested_headless {
        "headless"
    } else {
        "headed"
    };
    let current_mode = if handle.state.headless {
        "headless"
    } else {
        "headed"
    };
    bail!(
        "browser session `{}` is already running in {current_mode} mode. Stop it before restarting with {requested_mode} mode.",
        handle.state.session
    )
}

fn ensure_session_profile_matches(handle: &BrowserSessionHandle, profile_dir: &Path) -> Result<()> {
    let current = normalize_profile_dir(Path::new(&handle.state.profile_dir))?;
    let requested = normalize_profile_dir(profile_dir)?;
    if current == requested {
        return Ok(());
    }

    bail!(
        "browser session `{}` is already running with profile `{}`. Use a different `-s/--session`, or reuse this profile path.",
        handle.state.session,
        current.display(),
    )
}

fn resolve_session_profile_dir(
    global_home: &GlobalHome,
    session_name: &str,
    requested_profile_dir: Option<&Path>,
) -> Result<PathBuf> {
    match requested_profile_dir {
        Some(path) => normalize_profile_dir(path),
        None => Ok(global_home.browser_profile_dir(session_name)),
    }
}

fn normalize_profile_dir(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to resolve current directory for relative profile path")?
            .join(path))
    }
}

fn cleanup_stale_profile_singleton(profile_dir: &Path) -> Result<()> {
    let lock_path = profile_dir.join("SingletonLock");
    if !path_exists_without_following_symlink(&lock_path)? {
        return Ok(());
    }

    #[cfg(unix)]
    {
        if let Some(pid) = singleton_owner_pid(&lock_path) {
            if process_is_alive(pid) {
                bail!(
                    "browser profile `{}` is already in use by process {pid}. Close that browser instance or use a different session/profile",
                    profile_dir.display()
                );
            }
        }
    }

    for singleton_name in ["SingletonLock", "SingletonCookie", "SingletonSocket"] {
        let singleton_path = profile_dir.join(singleton_name);
        if path_exists_without_following_symlink(&singleton_path)? {
            fs::remove_file(&singleton_path)
                .with_context(|| format!("failed to remove stale {}", singleton_path.display()))?;
        }
    }

    Ok(())
}

fn path_exists_without_following_symlink(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err)
            .with_context(|| format!("failed to inspect filesystem entry {}", path.display())),
    }
}

#[cfg(unix)]
fn singleton_owner_pid(lock_path: &Path) -> Option<u32> {
    let link = fs::read_link(lock_path).ok()?;
    let owner = link.file_name()?.to_string_lossy();
    owner.rsplit('-').next()?.parse::<u32>().ok()
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn session_manifest_path(global_home: &GlobalHome, session_name: &str) -> PathBuf {
    global_home
        .browser_session_dir(session_name)
        .join(SESSION_FILE)
}

pub fn list_browser_sessions(global_home: &GlobalHome) -> Result<Vec<String>> {
    let sessions_dir = global_home.browser_sessions_dir();
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(&sessions_dir)
        .with_context(|| format!("failed to read {}", sessions_dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to inspect {}", sessions_dir.display()))?;
        if !entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }

        let session_name = entry.file_name().to_string_lossy().to_string();
        if validate_session_name(session_name.as_str()).is_err() {
            continue;
        }

        if entry.path().join(SESSION_FILE).exists() {
            names.push(session_name);
        }
    }

    names.sort();
    Ok(names)
}

fn load_state(
    global_home: &GlobalHome,
    session_name: &str,
) -> Result<Option<BrowserSessionHandle>> {
    let manifest_path = session_manifest_path(global_home, session_name);
    if !manifest_path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let state: BrowserSessionState = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    Ok(Some(BrowserSessionHandle {
        manifest_path,
        state,
    }))
}

fn save_state(path: &Path, state: &BrowserSessionState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(state).context("failed to serialize browser session")?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn remove_state(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn pick_free_port() -> Result<u16> {
    let listener =
        TcpListener::bind("127.0.0.1:0").context("failed to reserve a browser debugging port")?;
    let port = listener
        .local_addr()
        .context("failed to inspect reserved browser debugging port")?
        .port();
    drop(listener);
    Ok(port)
}

fn session_http_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn launch_browser_process(
    port: u16,
    profile_dir: &Path,
    log_path: &Path,
    headless: bool,
) -> Result<u32> {
    let executable = resolve_browser_executable()?;
    let args = browser_launch_args(port, profile_dir, headless);

    #[cfg(unix)]
    {
        launch_browser_process_via_shell(&executable, &args, log_path)
    }

    #[cfg(not(unix))]
    {
        let log_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .with_context(|| format!("failed to open browser log {}", log_path.display()))?;
        let log_file_err = log_file
            .try_clone()
            .with_context(|| format!("failed to clone browser log {}", log_path.display()))?;

        let mut command = Command::new(&executable);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err));

        let child = command
            .spawn()
            .with_context(|| format!("failed to launch Chromium from {}", executable.display()))?;

        Ok(child.id())
    }
}

fn resolve_browser_executable() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("OPENWALK_BROWSER_BIN") {
        return Ok(path.into());
    }

    chromiumoxide::detection::default_executable(DetectionOptions::default())
        .map_err(anyhow::Error::msg)
        .context(
            "failed to detect Chromium executable. Set `OPENWALK_BROWSER_BIN` if auto-detection cannot find your browser",
        )
}

fn browser_launch_args(port: u16, profile_dir: &Path, headless: bool) -> Vec<String> {
    let mut args = vec![
        format!("--remote-debugging-port={port}"),
        "--remote-debugging-address=127.0.0.1".to_string(),
        format!("--user-data-dir={}", profile_dir.display()),
        "--disable-dev-shm-usage".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--enable-automation".to_string(),
        "--password-store=basic".to_string(),
        "--use-mock-keychain".to_string(),
        "about:blank".to_string(),
    ];

    if env_flag_is_truthy("OPENWALK_NO_SANDBOX") {
        args.push("--no-sandbox".to_string());
        args.push("--disable-setuid-sandbox".to_string());
    }

    if headless {
        args.push("--headless".to_string());
        args.push("--hide-scrollbars".to_string());
        args.push("--mute-audio".to_string());
    }

    args
}

#[cfg(unix)]
fn launch_browser_process_via_shell(
    executable: &Path,
    args: &[String],
    log_path: &Path,
) -> Result<u32> {
    let executable = shell_quote(executable.to_string_lossy().as_ref());
    let args = args
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let log_path = shell_quote(log_path.to_string_lossy().as_ref());
    let script = format!("setsid {executable} {args} >> {log_path} 2>&1 < /dev/null & echo $!");

    let output = Command::new("sh")
        .arg("-lc")
        .arg(script)
        .output()
        .context("failed to launch Chromium through shell wrapper")?;

    if !output.status.success() {
        bail!(
            "shell wrapper failed to launch Chromium: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let pid = String::from_utf8(output.stdout)
        .context("failed to decode Chromium pid from shell wrapper")?
        .trim()
        .parse::<u32>()
        .context("failed to parse Chromium pid from shell wrapper")?;

    Ok(pid)
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

async fn wait_for_session_connect(
    http_url: &str,
    log_path: &Path,
    profile_dir: &Path,
) -> Result<(Browser, Handler)> {
    let deadline = tokio::time::Instant::now() + session_connect_timeout();
    loop {
        match try_connect_session(http_url).await? {
            Some(connected) => return Ok(connected),
            None if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(SESSION_CONNECT_POLL_INTERVAL).await;
            }
            None => {
                let mut message = format!(
                    "timed out waiting for browser session at {http_url}. Check browser log at {}",
                    log_path.display()
                );
                if profile_lock_conflict_detected(log_path) {
                    message.push_str(&format!(
                        ". Detected Chromium profile lock conflict; close the browser using profile `{}` or remove stale `Singleton*` entries",
                        profile_dir.display()
                    ));
                }
                bail!(message);
            }
        }
    }
}

fn profile_lock_conflict_detected(log_path: &Path) -> bool {
    let log = match fs::read_to_string(log_path) {
        Ok(content) => content,
        Err(_) => return false,
    };

    log.contains("ProcessSingleton")
        || log.contains("SingletonLock")
        || log.contains("profile appears to be in use")
}

async fn try_connect_session(http_url: &str) -> Result<Option<(Browser, Handler)>> {
    match tokio::time::timeout(
        session_connect_timeout(),
        Browser::connect_with_config(http_url.to_string(), browser_handler_config()),
    )
    .await
    {
        Ok(Ok(connected)) => Ok(Some(connected)),
        Ok(Err(_)) | Err(_) => Ok(None),
    }
}

fn browser_handler_config() -> HandlerConfig {
    HandlerConfig {
        request_timeout: browser_request_timeout(),
        ..HandlerConfig::default()
    }
}

struct SessionConnectDetails {
    ws_url: Option<String>,
    page_target_ids: Vec<String>,
}

async fn fetch_session_details(
    mut browser: Browser,
    mut handler: Handler,
) -> Result<SessionConnectDetails> {
    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    let ws_url = Some(browser.websocket_address().clone());
    let _ = browser.fetch_targets().await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let mut pages = browser.pages().await.unwrap_or_default();

    if pages.is_empty() {
        if let Ok(page) = browser.new_page("about:blank").await {
            pages.push(page);
        }
    }

    let page_target_ids = pages
        .iter()
        .map(|page| page.target_id().as_ref().to_string())
        .collect();

    drop(browser);
    handler_task.abort();
    let _ = handler_task.await;

    Ok(SessionConnectDetails {
        ws_url,
        page_target_ids,
    })
}

fn resolve_active_target_id(
    preferred_active_target_id: Option<&str>,
    page_target_ids: &[String],
) -> Option<String> {
    if let Some(preferred_active_target_id) = preferred_active_target_id {
        if page_target_ids
            .iter()
            .any(|target_id| target_id == preferred_active_target_id)
        {
            return Some(preferred_active_target_id.to_string());
        }
    }

    page_target_ids.first().cloned()
}

fn unix_timestamp_now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_secs())
}

impl BrowserActor {
    pub(super) async fn open(&mut self, url: String) -> Result<BrowserValue> {
        if self.browser.is_some() || !self.pages.is_empty() {
            bail!("browser is already open; call `browser-close` before `browser-open`");
        }

        self.ensure_browser_launched().await?;
        if self.has_single_placeholder_page().await? {
            if let Some(page) = self.pages.pop() {
                let page_id = page.target_id().as_ref().to_string();
                let _ = page.close().await;
                self.observed_network_targets.remove(page_id.as_str());
                self.clear_console_page_state(page_id.as_str());
            }
            self.active_page = None;
            self.persist_current_active_page().ok();
        }

        if !self.pages.is_empty() {
            bail!("browser is already open; call `browser-close` before `browser-open`");
        }

        let browser = self.browser.as_ref().expect("browser should be available");
        let page = browser
            .new_page("about:blank")
            .await
            .context("failed to create a fresh browser page")?;
        self.ensure_network_tracking_for_page(page.clone()).await?;
        self.ensure_console_tracking_for_page(page.clone()).await?;

        self.pages.push(page.clone());
        self.active_page = Some(self.pages.len() - 1);
        page.bring_to_front().await.ok();
        self.persist_current_active_page().ok();

        page.goto(url.as_str())
            .await
            .with_context(|| format!("failed to open a new page for `{url}`"))?;
        Ok(BrowserValue::String(page.url().await?.unwrap_or(url)))
    }

    async fn has_single_placeholder_page(&self) -> Result<bool> {
        if self.pages.len() != 1 {
            return Ok(false);
        }

        let page = self
            .pages
            .first()
            .expect("single-page check should guarantee a page exists");
        let current_url = page.url().await.unwrap_or(None).unwrap_or_default();
        Ok(matches!(
            current_url.as_str(),
            "" | "about:blank" | "chrome://newtab/" | "chrome://new-tab-page/"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, process,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::workspace::GlobalHome;

    static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let nonce = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "openwalk-browser-session-test-{}-{timestamp}-{nonce}",
                process::id()
            ));
            fs::create_dir_all(&path).expect("test temp dir should be created");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn browser_launch_args_include_debugging_port_and_profile() {
        let args = browser_launch_args(9222, Path::new("/tmp/openwalk-profile"), true);

        assert!(args
            .iter()
            .any(|item| item == "--remote-debugging-port=9222"));
        assert!(args
            .iter()
            .any(|item| item == "--user-data-dir=/tmp/openwalk-profile"));
    }

    #[test]
    fn browser_launch_args_skip_headless_flags_for_headed_mode() {
        let args = browser_launch_args(9222, Path::new("/tmp/openwalk-profile"), false);

        assert!(!args.iter().any(|item| item == "--headless"));
        assert!(!args.iter().any(|item| item == "--hide-scrollbars"));
        assert!(!args.iter().any(|item| item == "--mute-audio"));
    }

    #[test]
    fn session_name_validation_rejects_path_separator() {
        let err = validate_session_name("../default").expect_err("invalid name should fail");
        assert!(err.to_string().contains("invalid session name"));
    }

    #[test]
    fn discover_session_names_returns_sorted_manifest_dirs() {
        let sandbox = TestDir::new();
        let global_home = GlobalHome::from_root(sandbox.path.join("global-home"));
        global_home.init().expect("global home should initialize");

        let alpha_manifest = session_manifest_path(&global_home, "alpha");
        let bravo_manifest = session_manifest_path(&global_home, "bravo");
        let ignored_dir = global_home.browser_session_dir("no-manifest");
        fs::create_dir_all(&ignored_dir).expect("ignored session dir should exist");

        save_state(
            &bravo_manifest,
            &BrowserSessionState {
                session: "bravo".to_string(),
                pid: 2,
                port: 9223,
                http_url: "http://127.0.0.1:9223".to_string(),
                ws_url: None,
                profile_dir: "/tmp/bravo".to_string(),
                headless: true,
                active_target_id: None,
                started_at: 2,
            },
        )
        .expect("bravo state should be saved");
        save_state(
            &alpha_manifest,
            &BrowserSessionState {
                session: "alpha".to_string(),
                pid: 1,
                port: 9222,
                http_url: "http://127.0.0.1:9222".to_string(),
                ws_url: None,
                profile_dir: "/tmp/alpha".to_string(),
                headless: false,
                active_target_id: None,
                started_at: 1,
            },
        )
        .expect("alpha state should be saved");

        let names = list_browser_sessions(&global_home).expect("session names should load");
        assert_eq!(names, vec!["alpha".to_string(), "bravo".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn singleton_owner_pid_parses_pid_from_symlink_name() {
        let sandbox = TestDir::new();
        let lock = sandbox.path.join("SingletonLock");
        std::os::unix::fs::symlink("bloom-host-12345", &lock).expect("symlink should be created");

        assert_eq!(singleton_owner_pid(&lock), Some(12345));
    }

    #[cfg(unix)]
    #[test]
    fn path_exists_without_following_symlink_treats_dangling_symlink_as_existing() {
        let sandbox = TestDir::new();
        let dangling = sandbox.path.join("SingletonLock");
        std::os::unix::fs::symlink("missing-owner-999999", &dangling)
            .expect("dangling symlink should be created");

        assert!(
            path_exists_without_following_symlink(&dangling)
                .expect("symlink metadata lookup should work"),
            "dangling symlink should still be considered existing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_stale_profile_singleton_removes_dangling_singleton_entries() {
        let sandbox = TestDir::new();
        let lock = sandbox.path.join("SingletonLock");
        let cookie = sandbox.path.join("SingletonCookie");
        let socket = sandbox.path.join("SingletonSocket");

        std::os::unix::fs::symlink("stale-owner-999999", &lock)
            .expect("singleton lock symlink should be created");
        fs::write(&cookie, b"stale-cookie").expect("cookie marker should be created");
        fs::write(&socket, b"stale-socket").expect("socket marker should be created");

        cleanup_stale_profile_singleton(&sandbox.path)
            .expect("stale singleton cleanup should work");

        assert!(
            !path_exists_without_following_symlink(&lock).expect("lock path check should succeed"),
            "singleton lock should be removed"
        );
        assert!(
            !path_exists_without_following_symlink(&cookie)
                .expect("cookie path check should succeed"),
            "singleton cookie should be removed"
        );
        assert!(
            !path_exists_without_following_symlink(&socket)
                .expect("socket path check should succeed"),
            "singleton socket should be removed"
        );
    }

    #[test]
    fn resolve_active_target_id_prefers_existing_recorded_tab() {
        let target_ids = vec!["tab-a".to_string(), "tab-b".to_string()];

        let resolved = resolve_active_target_id(Some("tab-b"), &target_ids);

        assert_eq!(resolved.as_deref(), Some("tab-b"));
    }

    #[test]
    fn resolve_active_target_id_falls_back_to_first_available_tab() {
        let target_ids = vec!["tab-a".to_string(), "tab-b".to_string()];

        let resolved = resolve_active_target_id(Some("missing-tab"), &target_ids);

        assert_eq!(resolved.as_deref(), Some("tab-a"));
    }
}
