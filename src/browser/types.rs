use std::collections::BTreeMap;
use std::{collections::HashMap, path::PathBuf};

use super::session::BrowserSessionHandle;
use super::*;

#[derive(Debug, Clone)]
pub struct BrowserClient {
    pub(super) tx: UnboundedSender<BrowserRequest>,
}

#[derive(Debug)]
pub struct BrowserService {
    pub(super) client: BrowserClient,
    pub(super) task: JoinHandle<Result<()>>,
}

#[derive(Debug, Clone)]
pub(super) enum BrowserLaunchMode {
    Ephemeral(EphemeralLaunchOptions),
    Session(BrowserSessionHandle),
}

#[derive(Debug, Clone, Default)]
pub struct EphemeralLaunchOptions {
    pub profile_dir: Option<PathBuf>,
    pub headless: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum BrowserCommand {
    Open {
        url: String,
    },
    Goto {
        url: String,
    },
    Back,
    Forward,
    Reload,
    Click {
        selector: String,
    },
    DoubleClick {
        selector: String,
    },
    RightClick {
        selector: String,
    },
    Type {
        selector: String,
        text: String,
    },
    Fill {
        selector: String,
        text: String,
    },
    Press {
        key: String,
    },
    KeyboardType {
        text: String,
    },
    KeyboardDown {
        key: String,
    },
    KeyboardUp {
        key: String,
    },
    Select {
        selector: String,
        value: String,
    },
    Check {
        selector: String,
    },
    Uncheck {
        selector: String,
    },
    WaitTimeout {
        ms: u64,
    },
    WaitFunction {
        expression: String,
    },
    Exists {
        selector: String,
    },
    Hover {
        selector: String,
    },
    Screenshot {
        path: String,
    },
    ElementScreenshot {
        selector: String,
        path: String,
    },
    Pdf {
        path: String,
    },
    Eval {
        expression: String,
    },
    WaitNavigation,
    ScrollTo {
        x: i64,
        y: i64,
    },
    ScrollBy {
        x: i64,
        y: i64,
    },
    Viewport {
        width: i64,
        height: i64,
    },
    LocalStorageGet {
        key: String,
    },
    LocalStorageSet {
        key: String,
        value: String,
    },
    LocalStorageRemove {
        key: String,
    },
    LocalStorageClear,
    LocalStorageItems,
    SessionStorageGet {
        key: String,
    },
    SessionStorageSet {
        key: String,
        value: String,
    },
    SessionStorageRemove {
        key: String,
    },
    SessionStorageClear,
    SessionStorageItems,
    Cookies,
    CookieGet {
        name: String,
    },
    CookieSet {
        name: String,
        value: String,
        url: Option<String>,
        domain: Option<String>,
        path: Option<String>,
    },
    CookieDelete {
        name: String,
        url: Option<String>,
        domain: Option<String>,
        path: Option<String>,
    },
    CookiesClear,
    Tabs,
    NewTab {
        url: Option<String>,
    },
    SwitchTab {
        tab: String,
    },
    CloseTab {
        tab: Option<String>,
    },
    BrowserVersion,
    PerformanceMetrics,
    NetworkRequests,
    NetworkWaitResponse {
        url_contains: String,
    },
    NetworkResponseBody {
        url_contains: String,
    },
    ConsoleList,
    ConsoleClear,
    InspectInfo {
        selector: String,
    },
    InspectHighlight {
        selector: String,
    },
    InspectHideHighlight,
    InspectPick {
        timeout_ms: u64,
    },
    TracingStart {
        categories: Option<String>,
    },
    TracingStop {
        path: String,
    },
    MouseMove {
        x: f64,
        y: f64,
    },
    MouseClick {
        x: f64,
        y: f64,
    },
    MouseDown {
        x: f64,
        y: f64,
        button: MouseButton,
    },
    MouseUp {
        x: f64,
        y: f64,
        button: MouseButton,
    },
    MouseWheel {
        x: i64,
        y: i64,
        delta_x: f64,
        delta_y: f64,
    },
    TouchTap {
        x: i64,
        y: i64,
    },
    Cdp {
        method: String,
        params: String,
    },
    Close,
}

#[derive(Debug, Clone)]
pub enum BrowserValue {
    Unit,
    Boolean(bool),
    Number(i64),
    String(String),
    Array(Vec<BrowserValue>),
    Object(BTreeMap<String, BrowserValue>),
}

#[derive(Debug)]
pub(super) enum BrowserRequest {
    Command {
        command: BrowserCommand,
        respond_to: mpsc::Sender<Result<BrowserValue>>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum LocatorKind {
    Css,
    XPath,
}

#[derive(Debug, Clone)]
pub(super) enum Locator {
    Css(String),
    XPath(String),
}

impl Locator {
    pub(super) fn from_selector(selector: String) -> Self {
        let trimmed = selector.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("(//") || trimmed.starts_with(".//") {
            Self::XPath(selector)
        } else {
            Self::Css(selector)
        }
    }

    pub(super) fn css(selector: String) -> Self {
        Self::Css(selector)
    }

    pub(super) fn kind(&self) -> LocatorKind {
        match self {
            Self::Css(_) => LocatorKind::Css,
            Self::XPath(_) => LocatorKind::XPath,
        }
    }

    pub(super) fn raw(&self) -> &str {
        match self {
            Self::Css(value) | Self::XPath(value) => value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawCdpResult {
    pub(super) method: String,
    pub(super) result: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct BrowserTabInfo {
    pub(super) id: String,
    pub(super) url: String,
    pub(super) title: String,
    pub(super) active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct BrowserVersionInfo {
    pub(super) protocol_version: String,
    pub(super) product: String,
    pub(super) revision: String,
    pub(super) user_agent: String,
    pub(super) js_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct BrowserMetricsInfo {
    pub(super) url: String,
    pub(super) css_layout_viewport: serde_json::Value,
    pub(super) css_visual_viewport: serde_json::Value,
    pub(super) css_content_size: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NetworkRequestInfo {
    pub(super) url: String,
    pub(super) method: String,
    pub(super) document_url: String,
    pub(super) headers: serde_json::Value,
    pub(super) resource_type: Option<String>,
    pub(super) has_post_data: bool,
    pub(super) timestamp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NetworkResponseInfo {
    pub(super) url: String,
    pub(super) status: i64,
    pub(super) status_text: String,
    pub(super) mime_type: String,
    pub(super) headers: serde_json::Value,
    pub(super) resource_type: String,
    pub(super) remote_ip_address: Option<String>,
    pub(super) from_disk_cache: bool,
    pub(super) from_service_worker: bool,
    pub(super) encoded_data_length: f64,
    pub(super) timestamp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NetworkEntry {
    pub(super) page_id: String,
    pub(super) request_id: String,
    pub(super) request: NetworkRequestInfo,
    pub(super) response: Option<NetworkResponseInfo>,
    pub(super) finished: bool,
    pub(super) failed: bool,
    pub(super) failure_text: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct NetworkState {
    pub(super) entries: Vec<NetworkEntry>,
    pub(super) entry_index: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConsoleEntry {
    pub(super) sequence: u64,
    pub(super) page_id: String,
    pub(super) kind: String,
    pub(super) level: String,
    pub(super) text: String,
    pub(super) args: Vec<String>,
    pub(super) event_type: Option<String>,
    pub(super) source: Option<String>,
    pub(super) url: Option<String>,
    pub(super) line_number: Option<i64>,
    pub(super) column_number: Option<i64>,
    pub(super) context: Option<String>,
    pub(super) timestamp: f64,
}

#[derive(Debug, Default)]
pub(super) struct ConsoleState {
    pub(super) entries: Vec<ConsoleEntry>,
    pub(super) next_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct InspectNodeInfo {
    pub(super) page_id: String,
    pub(super) node_id: i64,
    pub(super) backend_node_id: i64,
    pub(super) node_type: i64,
    pub(super) node_name: String,
    pub(super) local_name: String,
    pub(super) node_value: String,
    pub(super) child_node_count: Option<i64>,
    pub(super) attributes: HashMap<String, String>,
    pub(super) frame_id: Option<String>,
    pub(super) is_svg: Option<bool>,
    pub(super) is_scrollable: Option<bool>,
    pub(super) outer_html: String,
    pub(super) bounding_box: Option<BoundingBoxInfo>,
}

#[derive(Debug, Clone)]
pub(super) struct TraceSession {
    pub(super) page_id: String,
    pub(super) categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TraceStartInfo {
    pub(super) page_id: String,
    pub(super) categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TraceStopInfo {
    pub(super) page_id: String,
    pub(super) path: String,
    pub(super) categories: Vec<String>,
    pub(super) bytes_written: usize,
    pub(super) data_loss_occurred: bool,
    pub(super) trace_format: Option<String>,
    pub(super) stream_compression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StorageEntry {
    pub(super) key: String,
    pub(super) value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct BoundingBoxInfo {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ClickKind {
    Single,
    Double,
    Right,
}

#[cfg(test)]
mod tests {
    use super::{Locator, LocatorKind};

    #[test]
    fn from_selector_defaults_to_css() {
        let locator = Locator::from_selector("div.card".to_string());
        assert!(matches!(locator.kind(), LocatorKind::Css));
        assert_eq!(locator.raw(), "div.card");
    }

    #[test]
    fn from_selector_detects_xpath_prefixes() {
        let locator = Locator::from_selector("//main//button".to_string());
        assert!(matches!(locator.kind(), LocatorKind::XPath));

        let locator = Locator::from_selector("(//a[@role='button'])[1]".to_string());
        assert!(matches!(locator.kind(), LocatorKind::XPath));

        let locator = Locator::from_selector("  .//span".to_string());
        assert!(matches!(locator.kind(), LocatorKind::XPath));
    }
}
