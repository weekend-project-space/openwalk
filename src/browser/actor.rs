use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use super::{
    session::BrowserSessionHandle,
    types::{
        BrowserClient, BrowserCommand, BrowserLaunchMode, BrowserRequest, BrowserService,
        BrowserValue, ClickKind, ConsoleState, Locator, NetworkState, TraceSession,
    },
    *,
};

#[derive(Debug)]
pub(super) struct BrowserActor {
    pub(super) mode: BrowserLaunchMode,
    pub(super) browser: Option<Browser>,
    pub(super) pages: Vec<Page>,
    pub(super) active_page: Option<usize>,
    pub(super) handler_task: Option<JoinHandle<()>>,
    pub(super) network_state: Arc<Mutex<NetworkState>>,
    pub(super) network_listener_tasks: Vec<JoinHandle<()>>,
    pub(super) observed_network_targets: HashSet<String>,
    pub(super) console_state: Arc<Mutex<ConsoleState>>,
    pub(super) console_listener_tasks: Vec<JoinHandle<()>>,
    pub(super) observed_console_targets: HashSet<String>,
    pub(super) trace_session: Option<TraceSession>,
}

impl BrowserService {
    pub fn spawn() -> Self {
        Self::spawn_ephemeral(Default::default())
    }

    pub fn spawn_ephemeral(options: super::types::EphemeralLaunchOptions) -> Self {
        Self::spawn_with_mode(BrowserLaunchMode::Ephemeral(options))
    }

    pub fn attach_session(session: BrowserSessionHandle) -> Self {
        Self::spawn_with_mode(BrowserLaunchMode::Session(session))
    }

    fn spawn_with_mode(mode: BrowserLaunchMode) -> Self {
        let (tx, rx) = unbounded_channel();
        let client = BrowserClient { tx };
        let task = tokio::spawn(async move { BrowserActor::new(mode).run(rx).await });
        Self { client, task }
    }

    pub fn client(&self) -> BrowserClient {
        self.client.clone()
    }

    pub async fn shutdown(self) -> Result<()> {
        let _ = self.client.tx.send(BrowserRequest::Shutdown);
        self.task.await.context("browser task failed to join")?
    }
}

impl BrowserActor {
    fn new(mode: BrowserLaunchMode) -> Self {
        Self {
            mode,
            browser: None,
            pages: Vec::new(),
            active_page: None,
            handler_task: None,
            network_state: Arc::new(Mutex::new(NetworkState::default())),
            network_listener_tasks: Vec::new(),
            observed_network_targets: HashSet::new(),
            console_state: Arc::new(Mutex::new(ConsoleState::default())),
            console_listener_tasks: Vec::new(),
            observed_console_targets: HashSet::new(),
            trace_session: None,
        }
    }
}

impl BrowserClient {
    pub fn call(&self, command: BrowserCommand) -> Result<BrowserValue> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(BrowserRequest::Command {
                command,
                respond_to: tx,
            })
            .map_err(|_| anyhow!("browser service is not available"))?;

        rx.recv()
            .context("browser service closed before responding")?
    }
}

impl BrowserActor {
    async fn run(mut self, mut rx: UnboundedReceiver<BrowserRequest>) -> Result<()> {
        while let Some(request) = rx.recv().await {
            match request {
                BrowserRequest::Command {
                    command,
                    respond_to,
                } => {
                    let result = self.handle(command).await;
                    let _ = respond_to.send(result);
                }
                BrowserRequest::Shutdown => break,
            }
        }

        self.shutdown_runtime().await
    }

    async fn handle(&mut self, command: BrowserCommand) -> Result<BrowserValue> {
        match command {
            BrowserCommand::Open { url } => self.open(url).await,
            BrowserCommand::Goto { url } => self.goto(url).await,
            BrowserCommand::Back => self.back().await,
            BrowserCommand::Forward => self.forward().await,
            BrowserCommand::Reload => self.reload().await,
            BrowserCommand::Click { selector } => {
                self.click_locator(Locator::from_selector(selector), ClickKind::Single)
                    .await
            }
            BrowserCommand::DoubleClick { selector } => {
                self.click_locator(Locator::from_selector(selector), ClickKind::Double)
                    .await
            }
            BrowserCommand::RightClick { selector } => {
                self.click_locator(Locator::from_selector(selector), ClickKind::Right)
                    .await
            }
            BrowserCommand::Type { selector, text } => {
                self.type_locator(Locator::from_selector(selector), text)
                    .await
            }
            BrowserCommand::Fill { selector, text } => {
                self.fill_locator(Locator::from_selector(selector), text)
                    .await
            }
            BrowserCommand::Press { key } => self.keyboard_press(key).await,
            BrowserCommand::KeyboardType { text } => self.keyboard_type(text).await,
            BrowserCommand::KeyboardDown { key } => self.keyboard_down(key).await,
            BrowserCommand::KeyboardUp { key } => self.keyboard_up(key).await,
            BrowserCommand::Select { selector, value } => {
                self.select_locator(Locator::from_selector(selector), value)
                    .await
            }
            BrowserCommand::Check { selector } => {
                self.set_checked_locator(Locator::from_selector(selector), true)
                    .await
            }
            BrowserCommand::Uncheck { selector } => {
                self.set_checked_locator(Locator::from_selector(selector), false)
                    .await
            }
            BrowserCommand::WaitTimeout { ms } => self.wait_timeout(ms).await,
            BrowserCommand::WaitFunction { expression } => self.wait_function(expression).await,
            BrowserCommand::Exists { selector } => {
                self.exists_locator(Locator::from_selector(selector)).await
            }
            BrowserCommand::Hover { selector } => {
                self.hover_locator(Locator::from_selector(selector)).await
            }
            BrowserCommand::Screenshot { path } => self.page_screenshot(path).await,
            BrowserCommand::ElementScreenshot { selector, path } => {
                self.element_screenshot_locator(Locator::from_selector(selector), path)
                    .await
            }
            BrowserCommand::Pdf { path } => self.page_pdf(path).await,
            BrowserCommand::Eval { expression } => self.eval(expression).await,
            BrowserCommand::WaitNavigation => self.wait_navigation().await,
            BrowserCommand::ScrollTo { x, y } => self.scroll_to(x, y).await,
            BrowserCommand::ScrollBy { x, y } => self.scroll_by(x, y).await,
            BrowserCommand::Viewport { width, height } => self.set_viewport(width, height).await,
            BrowserCommand::LocalStorageGet { key } => self.storage_get("localStorage", key).await,
            BrowserCommand::LocalStorageSet { key, value } => {
                self.storage_set("localStorage", key, value).await
            }
            BrowserCommand::LocalStorageRemove { key } => {
                self.storage_remove("localStorage", key).await
            }
            BrowserCommand::LocalStorageClear => self.storage_clear("localStorage").await,
            BrowserCommand::LocalStorageItems => self.storage_items("localStorage").await,
            BrowserCommand::SessionStorageGet { key } => {
                self.storage_get("sessionStorage", key).await
            }
            BrowserCommand::SessionStorageSet { key, value } => {
                self.storage_set("sessionStorage", key, value).await
            }
            BrowserCommand::SessionStorageRemove { key } => {
                self.storage_remove("sessionStorage", key).await
            }
            BrowserCommand::SessionStorageClear => self.storage_clear("sessionStorage").await,
            BrowserCommand::SessionStorageItems => self.storage_items("sessionStorage").await,
            BrowserCommand::Cookies => self.cookies().await,
            BrowserCommand::CookieGet { name } => self.cookie_get(name).await,
            BrowserCommand::CookieSet {
                name,
                value,
                url,
                domain,
                path,
            } => self.cookie_set(name, value, url, domain, path).await,
            BrowserCommand::CookieDelete {
                name,
                url,
                domain,
                path,
            } => self.cookie_delete(name, url, domain, path).await,
            BrowserCommand::CookiesClear => self.cookies_clear().await,
            BrowserCommand::Tabs => self.tabs().await,
            BrowserCommand::NewTab { url } => self.new_tab(url).await,
            BrowserCommand::SwitchTab { tab } => self.switch_tab(tab).await,
            BrowserCommand::CloseTab { tab } => self.close_tab(tab).await,
            BrowserCommand::BrowserVersion => self.browser_version().await,
            BrowserCommand::PerformanceMetrics => self.performance_metrics().await,
            BrowserCommand::NetworkRequests => self.network_requests().await,
            BrowserCommand::NetworkWaitResponse { url_contains } => {
                self.network_wait_response(url_contains).await
            }
            BrowserCommand::NetworkResponseBody { url_contains } => {
                self.network_response_body(url_contains).await
            }
            BrowserCommand::ConsoleList => self.console_list().await,
            BrowserCommand::ConsoleClear => self.console_clear().await,
            BrowserCommand::InspectInfo { selector } => {
                self.inspect_locator_info(Locator::css(selector)).await
            }
            BrowserCommand::InspectHighlight { selector } => {
                self.inspect_highlight_locator(Locator::css(selector)).await
            }
            BrowserCommand::InspectHideHighlight => self.inspect_hide_highlight().await,
            BrowserCommand::InspectPick { timeout_ms } => self.inspect_pick(timeout_ms).await,
            BrowserCommand::TracingStart { categories } => self.tracing_start(categories).await,
            BrowserCommand::TracingStop { path } => self.tracing_stop(path).await,
            BrowserCommand::MouseMove { x, y } => self.mouse_move(x, y).await,
            BrowserCommand::MouseClick { x, y } => self.mouse_click(x, y, 1).await,
            BrowserCommand::MouseDown { x, y, button } => self.mouse_down(x, y, button).await,
            BrowserCommand::MouseUp { x, y, button } => self.mouse_up(x, y, button).await,
            BrowserCommand::MouseWheel {
                x,
                y,
                delta_x,
                delta_y,
            } => self.mouse_wheel(x, y, delta_x, delta_y).await,
            BrowserCommand::TouchTap { x, y } => self.touch_tap(x, y).await,
            BrowserCommand::Cdp { method, params } => self.cdp(method, params).await,
            BrowserCommand::Close => {
                self.close_browser().await?;
                Ok(BrowserValue::Boolean(true))
            }
        }
    }
}
