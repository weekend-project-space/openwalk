use super::{
    actor::BrowserActor,
    session::connect_to_session,
    types::{BrowserLaunchMode, BrowserValue, BrowserVersionInfo},
    util::serialize_to_browser_value,
    *,
};
use std::{path::Path, process::Command, thread, time::Duration as StdDuration};

const OPENWALK_BROWSER_PROFILE_DIR_ENV: &str = "OPENWALK_BROWSER_PROFILE_DIR";

impl BrowserActor {
    pub(super) async fn ensure_browser_launched(&mut self) -> Result<()> {
        if self.browser.is_some() {
            return Ok(());
        }

        match &self.mode {
            BrowserLaunchMode::Ephemeral(options) => {
                self.launch_ephemeral_browser(options.clone()).await
            }
            BrowserLaunchMode::Session(handle) => {
                self.attach_persistent_browser(handle.clone()).await
            }
        }
    }

    async fn launch_ephemeral_browser(
        &mut self,
        options: super::types::EphemeralLaunchOptions,
    ) -> Result<()> {
        let mut builder = BrowserConfig::builder()
            .request_timeout(super::util::browser_request_timeout())
            .args([
                "--disable-breakpad",
                "--disable-crash-reporter",
                "--disable-crashpad-for-testing",
                "--disable-dev-shm-usage",
            ]);

        if let Some(path) = env::var_os("OPENWALK_BROWSER_BIN") {
            builder = builder.chrome_executable(path);
        }

        if super::util::env_flag_is_truthy("OPENWALK_NO_SANDBOX") {
            builder = builder.no_sandbox();
        }

        let profile_dir = if let Some(profile_dir) = options.profile_dir {
            profile_dir
        } else {
            resolve_browser_profile_dir()?
        };
        std::fs::create_dir_all(&profile_dir).with_context(|| {
            format!(
                "failed to create browser profile directory at {}",
                profile_dir.display()
            )
        })?;
        builder = builder.user_data_dir(&profile_dir);

        let headed = match options.headless {
            Some(headless) => !headless,
            None => {
                super::util::env_flag_is_truthy("OPENWALK_HEADFUL")
                    || super::util::env_flag_is_false("OPENWALK_HEADLESS")
            }
        };
        if headed {
            builder = builder.with_head();
        }

        let config = builder.build().map_err(anyhow::Error::msg)?;
        let (browser, mut handler) = Browser::launch(config).await.context(
            "failed to launch Chromium. Set `OPENWALK_BROWSER_BIN` if auto-detection cannot find your browser",
        )?;

        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if event.is_err() {
                    break;
                }
            }
        });

        self.browser = Some(browser);
        self.handler_task = Some(handler_task);

        Ok(())
    }

    async fn attach_persistent_browser(
        &mut self,
        session: super::session::BrowserSessionHandle,
    ) -> Result<()> {
        let (mut browser, mut handler) = connect_to_session(session.state()).await?;
        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if event.is_err() {
                    break;
                }
            }
        });

        browser
            .fetch_targets()
            .await
            .context("failed to fetch existing browser targets")?;
        tokio::time::sleep(Duration::from_millis(150)).await;

        self.browser = Some(browser);
        self.handler_task = Some(handler_task);
        self.refresh_pages_from_connected_browser().await?;

        if self.active_page.is_none() && self.pages.is_empty() {
            let browser = self.browser.as_ref().expect("browser should be available");
            let page = browser
                .new_page("about:blank")
                .await
                .context("failed to create a browser page for the session")?;
            self.ensure_network_tracking_for_page(page.clone()).await?;
            self.ensure_console_tracking_for_page(page.clone()).await?;
            self.pages.push(page);
            self.active_page = Some(0);
            self.persist_current_active_page()?;
        }

        self.mode = BrowserLaunchMode::Session(session);
        Ok(())
    }

    pub(super) async fn ensure_active_page(&mut self) -> Result<Page> {
        self.ensure_browser_launched().await?;

        if let Some(index) = self.active_page {
            if let Some(page) = self.pages.get(index) {
                return Ok(page.clone());
            }
        }

        let browser = self.browser.as_ref().expect("browser should be available");
        let page = browser
            .new_page("about:blank")
            .await
            .context("failed to create a browser page")?;
        self.pages.push(page.clone());
        self.active_page = Some(self.pages.len() - 1);
        self.persist_current_active_page().ok();

        Ok(page)
    }

    pub(super) fn require_page(&self) -> Result<Page> {
        self.active_page
            .and_then(|index| self.pages.get(index))
            .cloned()
            .ok_or_else(|| anyhow!("no active browser page. Call `browser-open` first"))
    }

    pub(super) async fn require_page_ready(&mut self) -> Result<Page> {
        self.ensure_browser_launched().await?;

        if let Ok(page) = self.require_page() {
            return Ok(page);
        }

        if matches!(self.mode, BrowserLaunchMode::Session(_)) {
            self.refresh_pages_from_connected_browser().await?;
            if let Ok(page) = self.require_page() {
                return Ok(page);
            }
        }

        bail!("no active browser page. Call `browser-open` first")
    }

    pub(super) async fn close_browser(&mut self) -> Result<()> {
        let session_pid = match &self.mode {
            BrowserLaunchMode::Session(session) => Some(session.state().pid),
            BrowserLaunchMode::Ephemeral(_) => None,
        };
        while let Some(page) = self.pages.pop() {
            let _ = page.close().await;
        }
        self.active_page = None;

        if let Some(mut browser) = self.browser.take() {
            let _ = browser.close().await;
        }

        if let Some(task) = self.handler_task.take() {
            let _ = task.await;
        }

        while let Some(task) = self.network_listener_tasks.pop() {
            let _ = task.await;
        }
        while let Some(task) = self.console_listener_tasks.pop() {
            let _ = task.await;
        }

        self.observed_network_targets.clear();
        self.observed_console_targets.clear();
        if let Ok(mut state) = self.network_state.lock() {
            state.entries.clear();
            state.entry_index.clear();
        }
        if let Ok(mut state) = self.console_state.lock() {
            state.entries.clear();
            state.next_sequence = 0;
        }
        self.trace_session = None;

        // When explicitly closing a persistent session browser, clear the
        // session manifest so `browser-list` reflects active/attachable sessions.
        if let BrowserLaunchMode::Session(session) = &self.mode {
            if let Some(pid) = session_pid {
                ensure_browser_process_stopped(pid)?;
            }
            session.remove_manifest()?;
        }

        Ok(())
    }

    pub(super) async fn detach_browser(&mut self) -> Result<()> {
        self.pages.clear();
        self.active_page = None;

        self.browser.take();

        if let Some(task) = self.handler_task.take() {
            task.abort();
            let _ = task.await;
        }

        while let Some(task) = self.network_listener_tasks.pop() {
            task.abort();
            let _ = task.await;
        }
        while let Some(task) = self.console_listener_tasks.pop() {
            task.abort();
            let _ = task.await;
        }

        self.observed_network_targets.clear();
        self.observed_console_targets.clear();
        if let Ok(mut state) = self.network_state.lock() {
            state.entries.clear();
            state.entry_index.clear();
        }
        if let Ok(mut state) = self.console_state.lock() {
            state.entries.clear();
            state.next_sequence = 0;
        }
        self.trace_session = None;

        Ok(())
    }

    pub(super) async fn shutdown_runtime(&mut self) -> Result<()> {
        match self.mode {
            BrowserLaunchMode::Ephemeral(_) => self.close_browser().await,
            BrowserLaunchMode::Session(_) => self.detach_browser().await,
        }
    }

    pub(super) async fn browser_version(&mut self) -> Result<BrowserValue> {
        self.ensure_browser_launched().await?;
        let browser = self.browser.as_ref().expect("browser should be available");
        let version = browser
            .version()
            .await
            .context("failed to read browser version")?;
        let info = BrowserVersionInfo {
            protocol_version: version.protocol_version,
            product: version.product,
            revision: version.revision,
            user_agent: version.user_agent,
            js_version: version.js_version,
        };
        serialize_to_browser_value(&info, "failed to serialize browser version")
    }
}

fn resolve_browser_profile_dir() -> Result<std::path::PathBuf> {
    if let Some(path) = env::var_os(OPENWALK_BROWSER_PROFILE_DIR_ENV) {
        return Ok(path.into());
    }

    let global_home = crate::workspace::GlobalHome::discover()?;
    global_home.init()?;
    Ok(global_home.default_browser_profile_dir())
}

#[cfg(unix)]
fn ensure_browser_process_stopped(pid: u32) -> Result<()> {
    if !process_is_alive(pid) {
        return Ok(());
    }

    let _ = Command::new("kill").arg(pid.to_string()).status();
    if wait_until_process_exits(pid, 2_000) {
        return Ok(());
    }

    let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
    if wait_until_process_exits(pid, 2_000) {
        return Ok(());
    }

    bail!("failed to stop browser process {pid}")
}

#[cfg(not(unix))]
fn ensure_browser_process_stopped(_pid: u32) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn wait_until_process_exits(pid: u32, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + StdDuration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if !process_is_alive(pid) {
            return true;
        }
        thread::sleep(StdDuration::from_millis(100));
    }
    !process_is_alive(pid)
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}
