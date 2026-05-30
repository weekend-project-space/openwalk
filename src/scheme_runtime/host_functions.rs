use std::cell::RefCell;

use anyhow::Result;
use scheme4r::{eval::Engine, runtime::BuiltinFn, Environment, SchemeError, Value};

use crate::workspace::GlobalHome;
use crate::{
    browser::{list_browser_sessions, parse_mouse_button, BrowserClient, BrowserCommand},
    value_codec::browser_value_to_scheme,
};

use super::errors::scheme_host_error;

thread_local! {
    static HOST_CONTEXT: RefCell<Option<HostContext>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct HostContext {
    browser: BrowserClient,
}

pub(super) struct HostContextGuard;

impl Drop for HostContextGuard {
    fn drop(&mut self) {
        HOST_CONTEXT.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

pub(super) fn install_host_context(browser: BrowserClient) -> HostContextGuard {
    HOST_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(HostContext { browser });
    });
    HostContextGuard
}

macro_rules! count_args {
    () => {
        0usize
    };
    ($($arg:ident),+ $(,)?) => {
        <[()]>::len(&[$(count_args!(@single $arg)),+])
    };
    (@single $_arg:ident) => {
        ()
    };
}

macro_rules! define_browser_builtin {
    ($fn_name:ident, $scheme_name:literal, [$($arg_name:ident => $parser:ident),* $(,)?], $command:expr) => {
        fn $fn_name(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
            #[allow(unused_mut, unused_assignments)]
            let mut index = 0usize;
            expect_arity($scheme_name, args, count_args!($($arg_name),*))?;
            $(
                let $arg_name = $parser($scheme_name, &args[index], stringify!($arg_name))?;
                index += 1;
            )*
            let _ = index;
            call_browser($command)
        }
    };
}

macro_rules! register_browser_builtins {
    ($env:expr, $( $name:literal => $func:ident ),* $(,)?) => {
        $(
            register_browser_builtin($env, $name, $func);
        )*
    };
}

pub(super) fn register_browser_builtins(env: &mut Environment) {
    register_browser_builtins!(
        env,
        "browser-open" => browser_open,
        "browser-list" => browser_list,
        "page-goto" => browser_goto,
        "page-back" => browser_back,
        "page-forward" => browser_forward,
        "page-reload" => browser_reload,
        "element-click" => browser_click,
        "element-double-click" => browser_double_click,
        "element-right-click" => browser_right_click,
        "element-type" => browser_type,
        "element-fill" => browser_fill,
        "keyboard-press" => browser_press,
        "keyboard-type" => browser_keyboard_type,
        "keyboard-down" => browser_keyboard_down,
        "keyboard-up" => browser_keyboard_up,
        "element-select" => browser_select,
        "element-check" => browser_check,
        "element-uncheck" => browser_uncheck,
        "time-sleep" => browser_wait_timeout,
        "js-wait" => browser_wait_function,
        "element-exists" => browser_exists,
        "element-hover" => browser_hover,
        "element-upload" => browser_upload,
        "element-drag" => browser_drag,
        "page-snapshot" => browser_page_snapshot,
        "page-screenshot" => browser_screenshot,
        "element-screenshot" => browser_element_screenshot,
        "page-pdf" => browser_pdf,
        "js-eval" => browser_eval,
        "page-wait-navigation" => browser_wait_navigation,
        "page-scroll-to" => browser_scroll_to,
        "page-scroll-by" => browser_scroll_by,
        "browser-resize" => browser_resize,
        "tab-list" => tab_list,
        "tab-new" => tab_new,
        "tab-select" => tab_select,
        "tab-close" => tab_close,
        "browser-version" => browser_version,
        "performance-metrics" => browser_performance_metrics,
        "network-log" => browser_network_log,
        "dialog-accept" => browser_dialog_accept,
        "dialog-dismiss" => browser_dialog_dismiss,
        "console" => browser_console,
        "console-clear" => browser_console_clear,
        "inspect-info" => browser_inspect_info,
        "inspect-highlight" => browser_inspect_highlight,
        "inspect-hide-highlight" => browser_inspect_hide_highlight,
        "inspect-pick" => browser_inspect_pick,
        "tracing-start" => browser_tracing_start,
        "tracing-stop" => browser_tracing_stop,
        "mouse-move" => browser_mouse_move,
        "mouse-click" => browser_mouse_click,
        "mouse-down" => browser_mouse_down,
        "mouse-up" => browser_mouse_up,
        "mouse-wheel" => browser_mouse_wheel,
        "touch-tap" => browser_touch_tap,
        "cdp-call" => browser_cdp,
        "browser-close" => browser_close,
    );
}

fn register_browser_builtin(env: &mut Environment, scheme_name: &str, func: BuiltinFn) {
    env.define(scheme_name, Value::builtin(scheme_name, func));
}

fn browser_open(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("browser-open", args, 1, 2)?;
    let url = expect_string("browser-open", &args[0], "url")?;
    let new_tab = match args.get(1) {
        Some(value) => expect_bool("browser-open", value, "new-tab")?,
        None => false,
    };
    call_browser(BrowserCommand::Open { url, new_tab })
}
define_browser_builtin!(browser_goto, "page-goto", [url => expect_string], BrowserCommand::Goto { url });
define_browser_builtin!(browser_back, "page-back", [], BrowserCommand::Back);
define_browser_builtin!(browser_forward, "page-forward", [], BrowserCommand::Forward);
define_browser_builtin!(browser_reload, "page-reload", [], BrowserCommand::Reload);
define_browser_builtin!(browser_click, "element-click", [selector => expect_string], BrowserCommand::Click { selector });
define_browser_builtin!(browser_double_click, "element-double-click", [selector => expect_string], BrowserCommand::DoubleClick { selector });
define_browser_builtin!(browser_right_click, "element-right-click", [selector => expect_string], BrowserCommand::RightClick { selector });
define_browser_builtin!(browser_type, "element-type", [selector => expect_string, text => expect_string], BrowserCommand::Type { selector, text });
define_browser_builtin!(browser_fill, "element-fill", [selector => expect_string, text => expect_string], BrowserCommand::Fill { selector, text });
define_browser_builtin!(browser_press, "keyboard-press", [key => expect_string], BrowserCommand::Press { key });
define_browser_builtin!(browser_keyboard_type, "keyboard-type", [text => expect_string], BrowserCommand::KeyboardType { text });
define_browser_builtin!(browser_keyboard_down, "keyboard-down", [key => expect_string], BrowserCommand::KeyboardDown { key });
define_browser_builtin!(browser_keyboard_up, "keyboard-up", [key => expect_string], BrowserCommand::KeyboardUp { key });
define_browser_builtin!(browser_select, "element-select", [selector => expect_string, value => expect_string], BrowserCommand::Select { selector, value });
define_browser_builtin!(browser_check, "element-check", [selector => expect_string], BrowserCommand::Check { selector });
define_browser_builtin!(browser_uncheck, "element-uncheck", [selector => expect_string], BrowserCommand::Uncheck { selector });
define_browser_builtin!(browser_wait_function, "js-wait", [expression => expect_string], BrowserCommand::WaitFunction { expression });
define_browser_builtin!(browser_exists, "element-exists", [selector => expect_string], BrowserCommand::Exists { selector });
define_browser_builtin!(browser_hover, "element-hover", [selector => expect_string], BrowserCommand::Hover { selector });
define_browser_builtin!(
    browser_drag,
    "element-drag",
    [source => expect_string, target => expect_string],
    BrowserCommand::Drag { source, target }
);
define_browser_builtin!(
    browser_page_snapshot,
    "page-snapshot",
    [],
    BrowserCommand::Snapshot
);
define_browser_builtin!(browser_screenshot, "page-screenshot", [path => expect_string], BrowserCommand::Screenshot { path });
define_browser_builtin!(browser_element_screenshot, "element-screenshot", [selector => expect_string, path => expect_string], BrowserCommand::ElementScreenshot { selector, path });
define_browser_builtin!(browser_pdf, "page-pdf", [path => expect_string], BrowserCommand::Pdf { path });
define_browser_builtin!(browser_eval, "js-eval", [expression => expect_string], BrowserCommand::Eval { expression });
define_browser_builtin!(
    browser_wait_navigation,
    "page-wait-navigation",
    [],
    BrowserCommand::WaitNavigation
);
define_browser_builtin!(tab_list, "tab-list", [], BrowserCommand::Tabs);
define_browser_builtin!(
    browser_version,
    "browser-version",
    [],
    BrowserCommand::BrowserVersion
);
define_browser_builtin!(
    browser_performance_metrics,
    "performance-metrics",
    [],
    BrowserCommand::PerformanceMetrics
);
define_browser_builtin!(
    browser_console_clear,
    "console-clear",
    [],
    BrowserCommand::ConsoleClear
);
define_browser_builtin!(
    browser_inspect_info,
    "inspect-info",
    [selector => expect_string],
    BrowserCommand::InspectInfo { selector }
);
define_browser_builtin!(
    browser_inspect_highlight,
    "inspect-highlight",
    [selector => expect_string],
    BrowserCommand::InspectHighlight { selector }
);
define_browser_builtin!(
    browser_inspect_hide_highlight,
    "inspect-hide-highlight",
    [],
    BrowserCommand::InspectHideHighlight
);
define_browser_builtin!(browser_close, "browser-close", [], BrowserCommand::Close);

pub(super) fn browser_list(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("browser-list", args, 0)?;
    let global_home = GlobalHome::discover().map_err(|err| {
        SchemeError::runtime(format!("failed to discover openwalk home: {err:#}"))
    })?;
    let sessions = list_browser_sessions(&global_home)
        .map_err(|err| SchemeError::runtime(format!("failed to list browser sessions: {err:#}")))?;
    Ok(Value::vector(
        sessions.into_iter().map(Value::string).collect(),
    ))
}

fn browser_wait_timeout(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("time-sleep", args, 1)?;
    let ms = expect_u64("time-sleep", &args[0], "ms")?;
    call_browser(BrowserCommand::WaitTimeout { ms })
}

fn browser_network_log(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("network-log", args, 0, 1)?;
    let url_contains = if let Some(value) = args.first() {
        Some(expect_string("network-log", value, "url_contains")?)
    } else {
        None
    };
    call_browser(BrowserCommand::NetworkLog { url_contains })
}

fn browser_dialog_accept(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("dialog-accept", args, 0, 1)?;
    let prompt_text = if let Some(value) = args.first() {
        Some(expect_string("dialog-accept", value, "prompt_text")?)
    } else {
        None
    };
    call_browser(BrowserCommand::DialogAccept { prompt_text })
}

define_browser_builtin!(
    browser_dialog_dismiss,
    "dialog-dismiss",
    [],
    BrowserCommand::DialogDismiss
);

fn browser_scroll_to(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("page-scroll-to", args, 2)?;
    let x = expect_i64("page-scroll-to", &args[0], "x")?;
    let y = expect_i64("page-scroll-to", &args[1], "y")?;
    call_browser(BrowserCommand::ScrollTo { x, y })
}

fn browser_scroll_by(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("page-scroll-by", args, 2)?;
    let x = expect_i64("page-scroll-by", &args[0], "x")?;
    let y = expect_i64("page-scroll-by", &args[1], "y")?;
    call_browser(BrowserCommand::ScrollBy { x, y })
}

fn browser_resize(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("browser-resize", args, 2)?;
    let width = expect_i64("browser-resize", &args[0], "width")?;
    let height = expect_i64("browser-resize", &args[1], "height")?;
    call_browser(BrowserCommand::Resize { width, height })
}

pub(super) fn browser_upload(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("element-upload", args, 2, usize::MAX)?;
    let selector = expect_string("element-upload", &args[0], "selector")?;
    let files = args
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, value)| expect_string("element-upload", value, &format!("file-{}", index)))
        .collect::<Result<Vec<_>, _>>()?;
    call_browser(BrowserCommand::Upload { selector, files })
}

pub(super) fn browser_console(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("console", args, 0, 1)?;
    let min_level = if args.is_empty() {
        None
    } else {
        Some(expect_string("console", &args[0], "min-level")?)
    };
    call_browser(BrowserCommand::Console { min_level })
}

fn tab_new(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("tab-new", args, 0, 1)?;
    let url = if args.is_empty() {
        None
    } else {
        Some(expect_string("tab-new", &args[0], "url")?)
    };
    call_browser(BrowserCommand::NewTab { url })
}

fn tab_select(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("tab-select", args, 1)?;
    let tab = expect_tab_reference("tab-select", &args[0], "tab")?;
    call_browser(BrowserCommand::SwitchTab { tab })
}

fn tab_close(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("tab-close", args, 0, 1)?;
    let tab = if args.is_empty() {
        None
    } else {
        Some(expect_tab_reference("tab-close", &args[0], "tab")?)
    };
    call_browser(BrowserCommand::CloseTab { tab })
}

fn browser_mouse_move(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("mouse-move", args, 2)?;
    let x = expect_f64("mouse-move", &args[0], "x")?;
    let y = expect_f64("mouse-move", &args[1], "y")?;
    call_browser(BrowserCommand::MouseMove { x, y })
}

fn browser_mouse_click(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("mouse-click", args, 2)?;
    let x = expect_f64("mouse-click", &args[0], "x")?;
    let y = expect_f64("mouse-click", &args[1], "y")?;
    call_browser(BrowserCommand::MouseClick { x, y })
}

fn browser_mouse_down(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("mouse-down", args, 3)?;
    let x = expect_f64("mouse-down", &args[0], "x")?;
    let y = expect_f64("mouse-down", &args[1], "y")?;
    let button = expect_mouse_button("mouse-down", &args[2], "button")?;
    call_browser(BrowserCommand::MouseDown { x, y, button })
}

fn browser_mouse_up(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("mouse-up", args, 3)?;
    let x = expect_f64("mouse-up", &args[0], "x")?;
    let y = expect_f64("mouse-up", &args[1], "y")?;
    let button = expect_mouse_button("mouse-up", &args[2], "button")?;
    call_browser(BrowserCommand::MouseUp { x, y, button })
}

fn browser_mouse_wheel(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("mouse-wheel", args, 4)?;
    let x = expect_i64("mouse-wheel", &args[0], "x")?;
    let y = expect_i64("mouse-wheel", &args[1], "y")?;
    let delta_x = expect_f64("mouse-wheel", &args[2], "delta-x")?;
    let delta_y = expect_f64("mouse-wheel", &args[3], "delta-y")?;
    call_browser(BrowserCommand::MouseWheel {
        x,
        y,
        delta_x,
        delta_y,
    })
}

fn browser_touch_tap(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("touch-tap", args, 2)?;
    let x = expect_i64("touch-tap", &args[0], "x")?;
    let y = expect_i64("touch-tap", &args[1], "y")?;
    call_browser(BrowserCommand::TouchTap { x, y })
}

fn browser_inspect_pick(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("inspect-pick", args, 0, 1)?;
    let timeout_ms = if args.is_empty() {
        30_000
    } else {
        expect_u64("inspect-pick", &args[0], "timeout-ms")?
    };
    call_browser(BrowserCommand::InspectPick { timeout_ms })
}

fn browser_tracing_start(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("tracing-start", args, 0, 1)?;
    let categories = if args.is_empty() {
        None
    } else {
        Some(expect_string("tracing-start", &args[0], "categories")?)
    };
    call_browser(BrowserCommand::TracingStart { categories })
}

fn browser_tracing_stop(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("tracing-stop", args, 1)?;
    let path = expect_string("tracing-stop", &args[0], "path")?;
    call_browser(BrowserCommand::TracingStop { path })
}

fn browser_cdp(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("cdp-call", args, 2)?;
    let method = expect_string("cdp-call", &args[0], "method")?;
    let params = expect_string("cdp-call", &args[1], "params")?;
    call_browser(BrowserCommand::Cdp { method, params })
}

fn call_browser(command: BrowserCommand) -> Result<Value, SchemeError> {
    with_host_context(|context| {
        context
            .browser
            .call(command)
            .map(browser_value_to_scheme)
            .map_err(scheme_host_error)
    })
}

fn with_host_context<T>(
    f: impl FnOnce(&HostContext) -> Result<T, SchemeError>,
) -> Result<T, SchemeError> {
    HOST_CONTEXT.with(|slot| {
        let slot = slot.borrow();
        let context = slot
            .as_ref()
            .ok_or_else(|| SchemeError::runtime("openwalk host context is not available"))?;
        f(context)
    })
}

fn expect_arity(name: &str, args: &[Value], expected: usize) -> Result<(), SchemeError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(SchemeError::arity(format!(
            "`{name}` expects {expected} argument(s), got {}",
            args.len()
        )))
    }
}

fn expect_arity_range(
    name: &str,
    args: &[Value],
    min: usize,
    max: usize,
) -> Result<(), SchemeError> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(SchemeError::arity(format!(
            "`{name}` expects between {min} and {max} argument(s), got {}",
            args.len()
        )))
    }
}

fn expect_string(name: &str, value: &Value, label: &str) -> Result<String, SchemeError> {
    match value {
        Value::String(text) => Ok(text.to_plain_string()),
        Value::Symbol(text) => Ok(text.clone()),
        other => Err(SchemeError::type_error(format!(
            "`{name}` expected `{label}` to be a string, got {other}"
        ))),
    }
}

fn expect_bool(name: &str, value: &Value, label: &str) -> Result<bool, SchemeError> {
    match value {
        Value::Boolean(value) => Ok(*value),
        other => Err(SchemeError::type_error(format!(
            "`{name}` expected `{label}` to be a boolean, got {other}"
        ))),
    }
}

fn expect_tab_reference(name: &str, value: &Value, label: &str) -> Result<String, SchemeError> {
    match value {
        Value::String(text) => Ok(text.to_plain_string()),
        Value::Symbol(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        other => Err(SchemeError::type_error(format!(
            "`{name}` expected `{label}` to be a tab index or id string, got {other}"
        ))),
    }
}

fn expect_i64(name: &str, value: &Value, label: &str) -> Result<i64, SchemeError> {
    match value {
        Value::Number(number) => Ok(*number),
        other => Err(SchemeError::type_error(format!(
            "`{name}` expected `{label}` to be a number, got {other}"
        ))),
    }
}

fn expect_u64(name: &str, value: &Value, label: &str) -> Result<u64, SchemeError> {
    let number = expect_i64(name, value, label)?;
    number.try_into().map_err(|_| {
        SchemeError::type_error(format!(
            "`{name}` expected `{label}` to be a non-negative number, got {number}"
        ))
    })
}

fn expect_f64(name: &str, value: &Value, label: &str) -> Result<f64, SchemeError> {
    Ok(expect_i64(name, value, label)? as f64)
}

fn expect_mouse_button(
    name: &str,
    value: &Value,
    label: &str,
) -> Result<chromiumoxide::cdp::browser_protocol::input::MouseButton, SchemeError> {
    let raw = expect_string(name, value, label)?;
    parse_mouse_button(raw.as_str()).map_err(|err| SchemeError::type_error(format!("{err:#}")))
}
