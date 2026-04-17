mod actor;
mod browser_ops;
mod devtools;
mod dom;
mod emulation;
mod input;
mod network;
mod page;
mod performance;
mod runtime;
mod session;
mod storage;
mod tab;
mod types;
mod util;

use std::{collections::HashMap, env, str::FromStr, sync::mpsc, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use chromiumoxide::cdp::browser_protocol::{
    emulation::SetDeviceMetricsOverrideParams,
    input::{
        DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams,
        DispatchMouseEventType, EmulateTouchFromMouseEventParams, EmulateTouchFromMouseEventType,
        MouseButton, SynthesizeScrollGestureParams,
    },
    network::{CookieParam, DeleteCookiesParams},
    page::{CaptureScreenshotFormat, GetLayoutMetricsParams, PrintToPdfParams},
    performance::GetMetricsParams,
};
use chromiumoxide::keys::get_key_definition;
use chromiumoxide::layout::Point;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::types::{ClickOptions, Command, Method, MethodId};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde::{Deserialize, Serialize, Serializer};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
    time::{sleep, Instant},
};

pub use session::{
    attach_browser_session_with_options, ensure_browser_session_with_options,
    list_browser_sessions, BrowserSessionLaunchOptions,
};
pub use types::{
    BrowserClient, BrowserCommand, BrowserService, BrowserValue, EphemeralLaunchOptions,
};
pub use util::parse_mouse_button;
