use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use scheme4r::{
    eval::Engine,
    runtime::{procedure::Procedure, BuiltinFn, EnvRef},
    Environment, Scheme, SchemeError, SchemeString, Value,
};

use crate::browser::{
    list_browser_sessions, parse_mouse_button, BrowserClient, BrowserCommand, BrowserValue,
};
use crate::tool_metadata::{ToolArgument, ToolMetadata, ToolReturn};
use crate::workspace::GlobalHome;

thread_local! {
    static HOST_CONTEXT: RefCell<Option<HostContext>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct HostContext {
    browser: BrowserClient,
}

struct HostContextGuard;

pub const SCHEME_BUILTINS: &[&str] = &[
    "browser-open",
    "browser-list",
    "page-goto",
    "page-back",
    "page-forward",
    "page-reload",
    "element-click",
    "element-double-click",
    "element-right-click",
    "element-type",
    "element-fill",
    "keyboard-press",
    "keyboard-type",
    "keyboard-down",
    "keyboard-up",
    "element-select",
    "element-check",
    "element-uncheck",
    "time-sleep",
    "js-wait",
    "element-exists",
    "element-hover",
    "page-screenshot",
    "element-screenshot",
    "page-pdf",
    "js-eval",
    "page-wait-navigation",
    "page-scroll-to",
    "page-scroll-by",
    "device-viewport",
    "localstorage-get",
    "localstorage-set",
    "localstorage-remove",
    "localstorage-clear",
    "localstorage-list",
    "sessionstorage-get",
    "sessionstorage-set",
    "sessionstorage-remove",
    "sessionstorage-clear",
    "sessionstorage-list",
    "cookie-list",
    "cookie-get",
    "cookie-set",
    "cookie-delete",
    "cookie-clear",
    "tab-list",
    "tab-new",
    "tab-select",
    "tab-close",
    "browser-version",
    "performance-metrics",
    "network-list",
    "network-wait-response",
    "network-response-body",
    "console-list",
    "console-clear",
    "inspect-info",
    "inspect-highlight",
    "inspect-hide-highlight",
    "inspect-pick",
    "tracing-start",
    "tracing-stop",
    "mouse-move",
    "mouse-click",
    "mouse-down",
    "mouse-up",
    "mouse-wheel",
    "touch-tap",
    "cdp-call",
    "browser-close",
];

impl Drop for HostContextGuard {
    fn drop(&mut self) {
        HOST_CONTEXT.with(|slot| {
            slot.borrow_mut().take();
        });
    }
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

pub async fn execute_script(
    script_path: &Path,
    args: &[String],
    browser: BrowserClient,
) -> Result<String> {
    let source = tokio::fs::read_to_string(script_path)
        .await
        .with_context(|| format!("failed to read script {}", script_path.display()))?;
    let script_path = script_path.to_path_buf();
    let args = args.to_vec();

    tokio::task::spawn_blocking(move || execute_script_sync(script_path, source, args, browser))
        .await
        .context("scheme execution task failed to join")?
}

pub async fn execute_builtin(
    name: &str,
    args: &[String],
    browser: BrowserClient,
) -> Result<String> {
    let name = name.to_string();
    let args = args.to_vec();

    tokio::task::spawn_blocking(move || execute_builtin_sync(name, args, browser))
        .await
        .context("builtin execution task failed to join")?
}

fn execute_builtin_sync(name: String, args: Vec<String>, browser: BrowserClient) -> Result<String> {
    if !SCHEME_BUILTINS.contains(&name.as_str()) {
        bail!("unknown builtin host function `{name}`");
    }

    let env = Environment::standard();
    let pseudo_path = PathBuf::from(format!("<builtin:{name}>"));
    install_openwalk_bindings(env.clone(), &pseudo_path, &args);

    let builtin = lookup_builtin_function(env.clone(), &name)?;
    let cli_args = cli_args_to_scheme_values(&name, &args)?;
    let engine = Engine::new(env);

    let _guard = install_host_context(HostContext { browser });
    let value = builtin(&engine, &cli_args)
        .map_err(|err| anyhow::anyhow!("builtin `{name}` execution failed: {err}"))?;

    Ok(scheme_value_to_json(&value).to_string())
}

pub fn builtin_tool_metadata(name: &str) -> Option<ToolMetadata> {
    if !SCHEME_BUILTINS.contains(&name) {
        return None;
    }

    Some(match name {
        "browser-open" => ToolMetadata {
            name: name.to_string(),
            description: "新开标签页并导航到指定 URL。".to_string(),
            args: vec![tool_arg("url", "string", true, "要打开的网页地址")],
            returns: ToolReturn {
                return_type: "string".to_string(),
                description: "新标签页最终打开的 URL。".to_string(),
            },
            examples: vec!["openwalk exec browser-open https://www.baidu.com".to_string()],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "browser".to_string(),
                "navigation".to_string(),
            ],
        },
        "browser-list" => ToolMetadata {
            name: name.to_string(),
            description: "列出当前环境已记录的浏览器会话名称。".to_string(),
            args: Vec::new(),
            returns: ToolReturn {
                return_type: "array".to_string(),
                description: "按字母排序的会话名称字符串数组。".to_string(),
            },
            examples: vec!["openwalk exec browser-list".to_string()],
            domains: Vec::new(),
            read_only: true,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "browser".to_string(),
                "session".to_string(),
            ],
        },
        "page-goto" => ToolMetadata {
            name: name.to_string(),
            description: "在当前活动标签页导航到指定 URL。".to_string(),
            args: vec![tool_arg("url", "string", true, "要导航到的网页地址")],
            returns: ToolReturn {
                return_type: "string".to_string(),
                description: "当前标签页最终打开的 URL。".to_string(),
            },
            examples: vec!["openwalk exec page-goto https://www.baidu.com".to_string()],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "page".to_string(),
                "navigation".to_string(),
            ],
        },
        "time-sleep" => ToolMetadata {
            name: name.to_string(),
            description: "等待指定毫秒数。".to_string(),
            args: vec![tool_arg("ms", "integer", true, "等待时长，单位毫秒")],
            returns: ToolReturn {
                return_type: "boolean".to_string(),
                description: "等待结束时返回 true。".to_string(),
            },
            examples: vec!["openwalk exec time-sleep 1000".to_string()],
            domains: Vec::new(),
            read_only: true,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "time".to_string(),
                "wait".to_string(),
            ],
        },
        "element-click" => ToolMetadata {
            name: name.to_string(),
            description: "点击匹配选择器的元素。".to_string(),
            args: vec![tool_arg("selector", "string", true, "CSS 选择器")],
            returns: ToolReturn {
                return_type: "unspecified".to_string(),
                description: "点击动作完成后返回未指定值。".to_string(),
            },
            examples: vec!["openwalk exec element-click \"#submit\"".to_string()],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "dom".to_string(),
                "input".to_string(),
            ],
        },
        "js-eval" => ToolMetadata {
            name: name.to_string(),
            description: "在当前页面执行一段 JavaScript。".to_string(),
            args: vec![tool_arg(
                "expression",
                "string",
                true,
                "要执行的 JavaScript 表达式",
            )],
            returns: ToolReturn {
                return_type: "any".to_string(),
                description: "表达式返回的可序列化结果。".to_string(),
            },
            examples: vec!["openwalk exec js-eval \"document.title\"".to_string()],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "runtime".to_string(),
                "javascript".to_string(),
            ],
        },
        "tab-list" => ToolMetadata {
            name: name.to_string(),
            description: "列出当前浏览器会话中的所有标签页。".to_string(),
            args: Vec::new(),
            returns: ToolReturn {
                return_type: "json-string".to_string(),
                description: "标签页数组的 JSON 字符串。".to_string(),
            },
            examples: vec!["openwalk exec tab-list".to_string()],
            domains: Vec::new(),
            read_only: true,
            requires_login: false,
            tags: vec!["builtin".to_string(), "tab".to_string(), "read".to_string()],
        },
        "tab-new" => ToolMetadata {
            name: name.to_string(),
            description: "创建一个新标签页，可选传入初始 URL。".to_string(),
            args: vec![tool_arg("url", "string", false, "新标签页初始打开的地址")],
            returns: ToolReturn {
                return_type: "json-string".to_string(),
                description: "新标签页信息的 JSON 字符串。".to_string(),
            },
            examples: vec!["openwalk exec tab-new https://www.baidu.com".to_string()],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "tab".to_string(),
                "navigation".to_string(),
            ],
        },
        "tab-select" => ToolMetadata {
            name: name.to_string(),
            description: "按标签页索引 idx 或 id（短 id，可用完整 id 前缀）切换到目标标签页。"
                .to_string(),
            args: vec![tool_arg(
                "tab",
                "string",
                true,
                "标签页 idx 或 id（可用 tab-list 查看短 id）",
            )],
            returns: ToolReturn {
                return_type: "json-string".to_string(),
                description: "切换后的标签页信息 JSON 字符串。".to_string(),
            },
            examples: vec![
                "openwalk exec tab-select 1".to_string(),
                "openwalk exec tab-select ABC1234".to_string(),
            ],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "tab".to_string(),
                "navigation".to_string(),
            ],
        },
        "tab-close" => ToolMetadata {
            name: name.to_string(),
            description: "关闭标签页；可选传入 idx 或 id（短 id），不传时关闭当前激活标签页。"
                .to_string(),
            args: vec![tool_arg(
                "tab",
                "string",
                false,
                "标签页 idx 或 id（可用 tab-list 查看短 id）",
            )],
            returns: ToolReturn {
                return_type: "boolean".to_string(),
                description: "关闭成功时返回 true。".to_string(),
            },
            examples: vec![
                "openwalk exec tab-close".to_string(),
                "openwalk exec tab-close 1".to_string(),
                "openwalk exec tab-close ABC1234".to_string(),
            ],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "tab".to_string(),
                "navigation".to_string(),
            ],
        },
        "network-list" => ToolMetadata {
            name: name.to_string(),
            description: "列出当前页面已记录的网络请求与响应。".to_string(),
            args: Vec::new(),
            returns: ToolReturn {
                return_type: "json-string".to_string(),
                description: "网络请求列表的 JSON 字符串。".to_string(),
            },
            examples: vec!["openwalk exec network-list".to_string()],
            domains: Vec::new(),
            read_only: true,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "network".to_string(),
                "read".to_string(),
            ],
        },
        "network-response-body" => ToolMetadata {
            name: name.to_string(),
            description: "按 URL 片段提取最近一次响应体。".to_string(),
            args: vec![tool_arg(
                "url_contains",
                "string",
                true,
                "用于匹配响应 URL 的片段",
            )],
            returns: ToolReturn {
                return_type: "string".to_string(),
                description: "匹配到的响应体内容。".to_string(),
            },
            examples: vec!["openwalk exec network-response-body api/search".to_string()],
            domains: Vec::new(),
            read_only: true,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "network".to_string(),
                "read".to_string(),
            ],
        },
        "cdp-call" => ToolMetadata {
            name: name.to_string(),
            description: "直接调用一条 CDP 命令。".to_string(),
            args: vec![
                tool_arg(
                    "method",
                    "string",
                    true,
                    "CDP 方法名，例如 Runtime.evaluate",
                ),
                tool_arg("params", "string", true, "JSON 字符串格式的参数对象"),
            ],
            returns: ToolReturn {
                return_type: "json-string".to_string(),
                description: "CDP 返回结果的 JSON 字符串。".to_string(),
            },
            examples: vec![
                r#"openwalk exec cdp-call Runtime.evaluate "{\"expression\":\"document.title\"}""#
                    .to_string(),
            ],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "cdp".to_string(),
                "devtools".to_string(),
            ],
        },
        "browser-close" => ToolMetadata {
            name: name.to_string(),
            description: "关闭当前浏览器会话。".to_string(),
            args: Vec::new(),
            returns: ToolReturn {
                return_type: "boolean".to_string(),
                description: "关闭成功时返回 true。".to_string(),
            },
            examples: vec!["openwalk exec browser-close".to_string()],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "browser".to_string(),
                "lifecycle".to_string(),
            ],
        },
        _ => default_builtin_tool_metadata(name),
    })
}

fn execute_script_sync(
    script_path: PathBuf,
    source: String,
    args: Vec<String>,
    browser: BrowserClient,
) -> Result<String> {
    let env = Environment::standard();
    install_openwalk_bindings(env.clone(), &script_path, &args);
    let scheme = Scheme::with_env(env);

    let _guard = install_host_context(HostContext { browser });
    let loaded_value = scheme
        .eval(&source)
        .map_err(|err| anyhow::anyhow!("scheme execution failed while loading script: {err}"))?;

    let value = match scheme.eval("(main openwalk-args)") {
        Ok(value) => value,
        Err(err) if is_missing_main(&err) => loaded_value,
        Err(err) => {
            return Err(anyhow::anyhow!(
                "scheme execution failed while calling `main`: {err}"
            ));
        }
    };

    Ok(scheme_value_to_json(&value).to_string())
}

fn install_host_context(context: HostContext) -> HostContextGuard {
    HOST_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(context);
    });
    HostContextGuard
}

fn install_openwalk_bindings(env: EnvRef, script_path: &Path, args: &[String]) {
    let mut env_ref = env.borrow_mut();

    env_ref.define(
        "openwalk-script-path",
        Value::string(script_path.display().to_string()),
    );
    env_ref.define(
        "openwalk-args",
        Value::list(args.iter().cloned().map(Value::string).collect()),
    );

    register_browser_builtins!(
        &mut env_ref,
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
        "page-screenshot" => browser_screenshot,
        "element-screenshot" => browser_element_screenshot,
        "page-pdf" => browser_pdf,
        "js-eval" => browser_eval,
        "page-wait-navigation" => browser_wait_navigation,
        "page-scroll-to" => browser_scroll_to,
        "page-scroll-by" => browser_scroll_by,
        "device-viewport" => browser_viewport,
        "localstorage-get" => browser_local_storage_get,
        "localstorage-set" => browser_local_storage_set,
        "localstorage-remove" => browser_local_storage_remove,
        "localstorage-clear" => browser_local_storage_clear,
        "localstorage-list" => browser_local_storage_items,
        "sessionstorage-get" => browser_session_storage_get,
        "sessionstorage-set" => browser_session_storage_set,
        "sessionstorage-remove" => browser_session_storage_remove,
        "sessionstorage-clear" => browser_session_storage_clear,
        "sessionstorage-list" => browser_session_storage_items,
        "cookie-list" => browser_cookies,
        "cookie-get" => browser_cookie_get,
        "cookie-set" => browser_cookie_set,
        "cookie-delete" => browser_cookie_delete,
        "cookie-clear" => browser_cookies_clear,
        "tab-list" => tab_list,
        "tab-new" => tab_new,
        "tab-select" => tab_select,
        "tab-close" => tab_close,
        "browser-version" => browser_version,
        "performance-metrics" => browser_performance_metrics,
        "network-list" => browser_network_requests,
        "network-wait-response" => browser_network_wait_response,
        "network-response-body" => browser_network_response_body,
        "console-list" => browser_console_list,
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

fn lookup_builtin_function(env: EnvRef, name: &str) -> Result<BuiltinFn> {
    let value = env
        .borrow()
        .lookup(name)
        .map_err(|err| anyhow::anyhow!("builtin `{name}` lookup failed: {err}"))?;

    match value {
        Value::Procedure(proc_ref) => match proc_ref.as_ref() {
            Procedure::Builtin { func, .. } => Ok(*func),
            _ => bail!("builtin `{name}` is not registered as a host builtin"),
        },
        _ => bail!("builtin `{name}` is not bound to a callable procedure"),
    }
}

fn cli_args_to_scheme_values(name: &str, args: &[String]) -> Result<Vec<Value>> {
    match name {
        "time-sleep" => args
            .iter()
            .map(|value| cli_number_arg(name, value, "ms"))
            .collect(),
        "inspect-pick" => args
            .iter()
            .map(|value| cli_number_arg(name, value, "timeout-ms"))
            .collect(),
        "page-scroll-to" | "page-scroll-by" | "device-viewport" => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_number_arg(name, value, cli_xy_label(index)))
            .collect(),
        "mouse-move" | "mouse-click" | "touch-tap" => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_number_arg(name, value, cli_xy_label(index)))
            .collect(),
        "mouse-down" | "mouse-up" => args
            .iter()
            .enumerate()
            .map(|(index, value)| {
                if index < 2 {
                    cli_number_arg(name, value, cli_xy_label(index))
                } else {
                    Ok(Value::string(value.clone()))
                }
            })
            .collect(),
        "mouse-wheel" => args
            .iter()
            .enumerate()
            .map(|(index, value)| cli_number_arg(name, value, cli_mouse_wheel_label(index)))
            .collect(),
        _ => Ok(args.iter().cloned().map(Value::string).collect()),
    }
}

fn cli_number_arg(name: &str, raw: &str, label: &str) -> Result<Value> {
    let value = raw.parse::<i64>().map_err(|_| {
        anyhow::anyhow!("`{name}` expected `{label}` to be an integer, got `{raw}`")
    })?;
    Ok(Value::Number(value))
}

fn cli_xy_label(index: usize) -> &'static str {
    match index {
        0 => "x",
        1 => "y",
        _ => "value",
    }
}

fn cli_mouse_wheel_label(index: usize) -> &'static str {
    match index {
        0 => "x",
        1 => "y",
        2 => "delta-x",
        3 => "delta-y",
        _ => "value",
    }
}

fn tool_arg(name: &str, arg_type: &str, required: bool, description: &str) -> ToolArgument {
    ToolArgument {
        name: name.to_string(),
        arg_type: arg_type.to_string(),
        required,
        default: None,
        description: description.to_string(),
    }
}

fn default_builtin_tool_metadata(name: &str) -> ToolMetadata {
    let domain = builtin_domain_tag(name);
    ToolMetadata {
        name: name.to_string(),
        description: format!("OpenWalk 内置 `{domain}` 领域指令 `{name}`。"),
        args: Vec::new(),
        returns: ToolReturn {
            return_type: "any".to_string(),
            description: "返回值取决于具体内置指令。".to_string(),
        },
        examples: Vec::new(),
        domains: Vec::new(),
        read_only: false,
        requires_login: false,
        tags: vec!["builtin".to_string(), domain.to_string()],
    }
}

fn builtin_domain_tag(name: &str) -> &'static str {
    if name.starts_with("page-") {
        "page"
    } else if name.starts_with("element-") {
        "dom"
    } else if name.starts_with("keyboard-")
        || name.starts_with("mouse-")
        || name.starts_with("touch-")
    {
        "input"
    } else if name.starts_with("tab-") {
        "tab"
    } else if name.starts_with("network-") {
        "network"
    } else if name.starts_with("console-") {
        "console"
    } else if name.starts_with("inspect-") {
        "inspect"
    } else if name.starts_with("tracing-") {
        "tracing"
    } else if name.starts_with("device-") {
        "device"
    } else if name.starts_with("localstorage-") || name.starts_with("sessionstorage-") {
        "storage"
    } else if name.starts_with("cookie-") {
        "cookie"
    } else if name.starts_with("browser-") {
        "browser"
    } else if name.starts_with("js-") {
        "runtime"
    } else if name.starts_with("time-") {
        "time"
    } else if name.starts_with("cdp-") {
        "cdp"
    } else {
        "browser"
    }
}

fn is_missing_main(err: &SchemeError) -> bool {
    err.to_string().contains("undefined variable: main")
}

fn register_browser_builtin(env: &mut scheme4r::Environment, scheme_name: &str, func: BuiltinFn) {
    env.define(scheme_name, Value::builtin(scheme_name, func));
}

define_browser_builtin!(browser_open, "browser-open", [url => expect_string], BrowserCommand::Open { url });
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
define_browser_builtin!(browser_local_storage_get, "localstorage-get", [key => expect_string], BrowserCommand::LocalStorageGet { key });
define_browser_builtin!(browser_local_storage_set, "localstorage-set", [key => expect_string, value => expect_string], BrowserCommand::LocalStorageSet { key, value });
define_browser_builtin!(browser_local_storage_remove, "localstorage-remove", [key => expect_string], BrowserCommand::LocalStorageRemove { key });
define_browser_builtin!(
    browser_local_storage_clear,
    "localstorage-clear",
    [],
    BrowserCommand::LocalStorageClear
);
define_browser_builtin!(
    browser_local_storage_items,
    "localstorage-list",
    [],
    BrowserCommand::LocalStorageItems
);
define_browser_builtin!(browser_session_storage_get, "sessionstorage-get", [key => expect_string], BrowserCommand::SessionStorageGet { key });
define_browser_builtin!(browser_session_storage_set, "sessionstorage-set", [key => expect_string, value => expect_string], BrowserCommand::SessionStorageSet { key, value });
define_browser_builtin!(browser_session_storage_remove, "sessionstorage-remove", [key => expect_string], BrowserCommand::SessionStorageRemove { key });
define_browser_builtin!(
    browser_session_storage_clear,
    "sessionstorage-clear",
    [],
    BrowserCommand::SessionStorageClear
);
define_browser_builtin!(
    browser_session_storage_items,
    "sessionstorage-list",
    [],
    BrowserCommand::SessionStorageItems
);
define_browser_builtin!(browser_cookies, "cookie-list", [], BrowserCommand::Cookies);
define_browser_builtin!(browser_cookie_get, "cookie-get", [name => expect_string], BrowserCommand::CookieGet { name });
define_browser_builtin!(
    browser_cookies_clear,
    "cookie-clear",
    [],
    BrowserCommand::CookiesClear
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
    browser_network_requests,
    "network-list",
    [],
    BrowserCommand::NetworkRequests
);
define_browser_builtin!(
    browser_network_wait_response,
    "network-wait-response",
    [url_contains => expect_string],
    BrowserCommand::NetworkWaitResponse { url_contains }
);
define_browser_builtin!(
    browser_network_response_body,
    "network-response-body",
    [url_contains => expect_string],
    BrowserCommand::NetworkResponseBody { url_contains }
);
define_browser_builtin!(
    browser_console_list,
    "console-list",
    [],
    BrowserCommand::ConsoleList
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

fn browser_list(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
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

fn browser_viewport(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity("device-viewport", args, 2)?;
    let width = expect_i64("device-viewport", &args[0], "width")?;
    let height = expect_i64("device-viewport", &args[1], "height")?;
    call_browser(BrowserCommand::Viewport { width, height })
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

fn browser_cookie_set(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("cookie-set", args, 2, 5)?;
    let name = expect_string("cookie-set", &args[0], "name")?;
    let value = expect_string("cookie-set", &args[1], "value")?;
    let url = optional_string_arg("cookie-set", args, 2, "url")?;
    let domain = optional_string_arg("cookie-set", args, 3, "domain")?;
    let path = optional_string_arg("cookie-set", args, 4, "path")?;
    call_browser(BrowserCommand::CookieSet {
        name,
        value,
        url,
        domain,
        path,
    })
}

fn browser_cookie_delete(_: &Engine, args: &[Value]) -> Result<Value, SchemeError> {
    expect_arity_range("cookie-delete", args, 1, 4)?;
    let name = expect_string("cookie-delete", &args[0], "name")?;
    let url = optional_string_arg("cookie-delete", args, 1, "url")?;
    let domain = optional_string_arg("cookie-delete", args, 2, "domain")?;
    let path = optional_string_arg("cookie-delete", args, 3, "path")?;
    call_browser(BrowserCommand::CookieDelete {
        name,
        url,
        domain,
        path,
    })
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
            .map_err(|err| SchemeError::runtime(format!("{err:#}")))
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

fn optional_string_arg(
    name: &str,
    args: &[Value],
    index: usize,
    label: &str,
) -> Result<Option<String>, SchemeError> {
    if let Some(value) = args.get(index) {
        Ok(Some(expect_string(name, value, label)?))
    } else {
        Ok(None)
    }
}

fn browser_value_to_scheme(value: BrowserValue) -> Value {
    match value {
        BrowserValue::Unit => Value::Unspecified,
        BrowserValue::Boolean(value) => Value::Boolean(value),
        BrowserValue::Number(value) => Value::Number(value),
        BrowserValue::String(value) => Value::String(SchemeString::new(value)),
        BrowserValue::Array(values) => {
            Value::vector(values.into_iter().map(browser_value_to_scheme).collect())
        }
        BrowserValue::Object(values) => Value::list(
            values
                .into_iter()
                .map(|(key, value)| Value::pair(Value::string(key), browser_value_to_scheme(value)))
                .collect(),
        ),
    }
}

fn scheme_value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Boolean(value) => serde_json::Value::Bool(*value),
        Value::Number(value) => serde_json::Value::Number((*value).into()),
        Value::Character(value) => serde_json::Value::String(value.to_string()),
        Value::String(value) => serde_json::Value::String(value.to_plain_string()),
        Value::Symbol(value) => serde_json::Value::String(value.clone()),
        Value::Vector(values) => serde_json::Value::Array(
            values
                .borrow()
                .iter()
                .map(scheme_value_to_json)
                .collect::<Vec<_>>(),
        ),
        Value::ByteVector(values) => serde_json::Value::Array(
            values
                .borrow()
                .iter()
                .map(|value| serde_json::Value::Number(i64::from(*value).into()))
                .collect::<Vec<_>>(),
        ),
        Value::Dict(values) => {
            let mut map = serde_json::Map::new();
            for (key, value) in values.borrow().iter() {
                map.insert(dict_key_to_string(key), scheme_value_to_json(value));
            }
            serde_json::Value::Object(map)
        }
        Value::Record(record) => {
            let record = record.borrow();
            let record_type = record.record_type();
            let mut fields = serde_json::Map::new();
            for index in 0..record_type.field_count() {
                let field_name = record_type
                    .field_name(index)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("field-{index}"));
                if let Some(value) = record.field(index) {
                    fields.insert(field_name, scheme_value_to_json(value));
                }
            }

            let mut map = serde_json::Map::new();
            map.insert(
                "$record".to_string(),
                serde_json::Value::String(record_type.name().to_string()),
            );
            map.insert("fields".to_string(), serde_json::Value::Object(fields));
            serde_json::Value::Object(map)
        }
        Value::Pair(_) | Value::EmptyList => list_or_pair_to_json(value),
        Value::Multiple(values) => {
            serde_json::Value::Array(values.iter().map(scheme_value_to_json).collect())
        }
        Value::Unspecified | Value::EofObject => serde_json::Value::Null,
        Value::Port(_)
        | Value::ErrorObject(_)
        | Value::Parameter(_)
        | Value::Continuation(_)
        | Value::Procedure(_) => serde_json::Value::String(value.to_string()),
    }
}

fn dict_key_to_string(key: &scheme4r::runtime::DictKey) -> String {
    match key {
        scheme4r::runtime::DictKey::Boolean(value) => value.to_string(),
        scheme4r::runtime::DictKey::Number(value) => value.to_string(),
        scheme4r::runtime::DictKey::Character(value) => value.to_string(),
        scheme4r::runtime::DictKey::String(value) => value.clone(),
        scheme4r::runtime::DictKey::Symbol(value) => value.clone(),
        scheme4r::runtime::DictKey::EmptyList => "()".to_string(),
    }
}

fn list_or_pair_to_json(value: &Value) -> serde_json::Value {
    if let Some(items) = value.to_proper_list_vec() {
        if let Some(object) = maybe_alist_to_json_object(items.as_slice()) {
            return serde_json::Value::Object(object);
        }
        return serde_json::Value::Array(items.iter().map(scheme_value_to_json).collect());
    }

    if let Value::Pair(pair) = value {
        let pair = pair.borrow();
        let mut map = serde_json::Map::new();
        map.insert("car".to_string(), scheme_value_to_json(&pair.car));
        map.insert("cdr".to_string(), scheme_value_to_json(&pair.cdr));
        return serde_json::Value::Object(map);
    }

    serde_json::Value::String(value.to_string())
}

fn maybe_alist_to_json_object(
    items: &[Value],
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut map = serde_json::Map::new();
    for item in items {
        let Value::Pair(entry) = item else {
            return None;
        };
        let entry = entry.borrow();
        let key = match &entry.car {
            Value::String(value) => value.to_plain_string(),
            Value::Symbol(value) => value.clone(),
            _ => return None,
        };
        map.insert(key, scheme_value_to_json(&entry.cdr));
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs, process,
        ffi::OsString,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "openwalk-scheme-test-{}-{timestamp}",
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

    #[tokio::test]
    async fn execute_script_runs_plain_scheme_without_browser() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("math.scm");
        fs::write(&script_path, "(define (main args) (+ 1 2 3))")
            .expect("script should be written");

        let browser = crate::browser::BrowserService::spawn();
        let result = execute_script(&script_path, &[], browser.client())
            .await
            .expect("script should execute");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert_eq!(result, "6");
    }

    #[tokio::test]
    async fn execute_script_exposes_openwalk_args() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("args.scm");
        fs::write(&script_path, "(define (main args) (car args))")
            .expect("script should be written");

        let browser = crate::browser::BrowserService::spawn();
        let result = execute_script(&script_path, &[String::from("hello")], browser.client())
            .await
            .expect("script should execute");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert_eq!(result, "\"hello\"");
    }

    #[tokio::test]
    async fn execute_script_calls_main_with_cli_args() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("main.scm");
        fs::write(
            &script_path,
            "(define (main args) (if (null? args) \"empty\" (car args)))",
        )
        .expect("script should be written");

        let browser = crate::browser::BrowserService::spawn();
        let result = execute_script(&script_path, &[String::from("from-cli")], browser.client())
            .await
            .expect("script should execute");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert_eq!(result, "\"from-cli\"");
    }

    #[tokio::test]
    async fn execute_script_falls_back_to_top_level_value_without_main() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("fallback.scm");
        fs::write(&script_path, "(+ 40 2)").expect("script should be written");

        let browser = crate::browser::BrowserService::spawn();
        let result = execute_script(&script_path, &[], browser.client())
            .await
            .expect("script should execute");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert_eq!(result, "42");
    }

    #[tokio::test]
    async fn execute_script_supports_domain_style_builtins() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("domain-names.scm");
        fs::write(
            &script_path,
            "(define (main args) (if (time-sleep 0) \"domain-ok\" \"domain-bad\"))",
        )
        .expect("script should be written");

        let browser = crate::browser::BrowserService::spawn();
        let result = execute_script(&script_path, &[], browser.client())
            .await
            .expect("script should execute");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert_eq!(result, "\"domain-ok\"");
    }

    #[tokio::test]
    async fn execute_script_rejects_legacy_browser_builtin_names() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("legacy-name.scm");
        fs::write(&script_path, "(define (main args) browser-goto)")
            .expect("script should be written");

        let browser = crate::browser::BrowserService::spawn();
        let error = execute_script(&script_path, &[], browser.client())
            .await
            .expect_err("legacy names should no longer be registered");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert!(error
            .to_string()
            .contains("undefined variable: browser-goto"));
    }

    #[tokio::test]
    async fn execute_script_rejects_legacy_browser_tab_builtin_names() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("legacy-tab-name.scm");
        fs::write(&script_path, "(define (main args) browser-tabs)")
            .expect("script should be written");

        let browser = crate::browser::BrowserService::spawn();
        let error = execute_script(&script_path, &[], browser.client())
            .await
            .expect_err("legacy tab names should no longer be registered");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert!(error
            .to_string()
            .contains("undefined variable: browser-tabs"));
    }

    #[tokio::test]
    async fn execute_script_rejects_pre_refactor_scheme_names() {
        let sandbox = TestDir::new();
        let script_path = sandbox.path.join("legacy-domain-name.scm");
        fs::write(&script_path, "(define (main args) input-click)")
            .expect("script should be written");

        let browser = crate::browser::BrowserService::spawn();
        let error = execute_script(&script_path, &[], browser.client())
            .await
            .expect_err("old scheme names should no longer be registered");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert!(error
            .to_string()
            .contains("undefined variable: input-click"));
    }

    #[test]
    fn builtin_tool_metadata_exposes_browser_open() {
        let metadata =
            builtin_tool_metadata("browser-open").expect("browser-open metadata should exist");

        assert_eq!(metadata.name, "browser-open");
        assert_eq!(metadata.description, "新开标签页并导航到指定 URL。");
        assert_eq!(metadata.returns.return_type, "string");
        assert_eq!(metadata.args.len(), 1);
        assert_eq!(metadata.args[0].name, "url");
    }

    #[test]
    fn builtin_tool_metadata_exposes_browser_list() {
        let metadata =
            builtin_tool_metadata("browser-list").expect("browser-list metadata should exist");

        assert_eq!(metadata.name, "browser-list");
        assert_eq!(metadata.description, "列出当前环境已记录的浏览器会话名称。");
        assert_eq!(metadata.returns.return_type, "array");
        assert!(metadata.args.is_empty());
        assert!(metadata.read_only);
    }

    #[test]
    fn browser_list_returns_scheme_vector() {
        let _env_guard = ENV_LOCK.lock().expect("env lock should be acquired");
        let sandbox = TestDir::new();
        let global_home_root = sandbox.path.join("global-home");
        let _openwalk_home =
            EnvVarGuard::set("OPENWALK_HOME", global_home_root.to_str().expect("utf8 path"));

        let global_home = GlobalHome::discover().expect("global home should resolve");
        global_home.init().expect("global home should initialize");

        for session_name in ["qa", "default"] {
            let session_dir = global_home.browser_session_dir(session_name);
            fs::create_dir_all(&session_dir).expect("session dir should be created");
            fs::write(session_dir.join("session.json"), "{}")
                .expect("session manifest should be written");
        }

        let value = browser_list(&Engine::new(Environment::standard()), &[])
            .expect("browser-list should return a value");

        let Value::Vector(items) = value else {
            panic!("browser-list should return a vector");
        };
        let items = items.borrow();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items.iter().map(expect_test_string).collect::<Vec<_>>(),
            vec!["default".to_string(), "qa".to_string()]
        );
    }

    #[test]
    fn builtin_tool_metadata_exposes_page_goto() {
        let metadata = builtin_tool_metadata("page-goto").expect("page-goto metadata should exist");

        assert_eq!(metadata.name, "page-goto");
        assert_eq!(metadata.description, "在当前活动标签页导航到指定 URL。");
        assert_eq!(metadata.returns.return_type, "string");
        assert_eq!(metadata.args.len(), 1);
        assert_eq!(metadata.args[0].name, "url");
    }

    #[tokio::test]
    async fn execute_builtin_supports_cli_number_arguments() {
        let browser = crate::browser::BrowserService::spawn();
        let result = execute_builtin("time-sleep", &[String::from("0")], browser.client())
            .await
            .expect("builtin should execute");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        assert_eq!(result, "true");
    }

    #[tokio::test]
    async fn execute_builtin_tab_new_requires_browser_open_first() {
        let browser = crate::browser::BrowserService::spawn();
        let error = execute_builtin("tab-new", &[], browser.client())
            .await
            .expect_err("tab-new should require browser-open first");
        browser
            .shutdown()
            .await
            .expect("browser service should stop");

        let message = error.to_string();
        assert!(message.contains("tab-new"));
        assert!(message.contains("browser-open"));
    }

    #[test]
    fn scheme_builtin_list_is_large_enough() {
        assert!(SCHEME_BUILTINS.len() >= 60);
    }

    #[test]
    fn scheme_builtin_list_contains_tab_helpers() {
        assert!(SCHEME_BUILTINS.contains(&"tab-list"));
        assert!(SCHEME_BUILTINS.contains(&"tab-new"));
        assert!(SCHEME_BUILTINS.contains(&"tab-select"));
        assert!(SCHEME_BUILTINS.contains(&"tab-close"));
    }

    #[test]
    fn scheme_builtin_list_contains_network_helpers() {
        assert!(SCHEME_BUILTINS.contains(&"network-list"));
        assert!(SCHEME_BUILTINS.contains(&"network-wait-response"));
        assert!(SCHEME_BUILTINS.contains(&"network-response-body"));
    }

    #[test]
    fn scheme_builtin_list_contains_refactored_domain_helpers() {
        assert!(SCHEME_BUILTINS.contains(&"browser-list"));
        assert!(SCHEME_BUILTINS.contains(&"page-goto"));
        assert!(SCHEME_BUILTINS.contains(&"element-click"));
        assert!(SCHEME_BUILTINS.contains(&"keyboard-press"));
        assert!(SCHEME_BUILTINS.contains(&"mouse-click"));
        assert!(SCHEME_BUILTINS.contains(&"touch-tap"));
        assert!(SCHEME_BUILTINS.contains(&"js-eval"));
        assert!(SCHEME_BUILTINS.contains(&"js-wait"));
        assert!(SCHEME_BUILTINS.contains(&"time-sleep"));
        assert!(SCHEME_BUILTINS.contains(&"device-viewport"));
        assert!(SCHEME_BUILTINS.contains(&"cookie-list"));
        assert!(SCHEME_BUILTINS.contains(&"localstorage-get"));
        assert!(SCHEME_BUILTINS.contains(&"sessionstorage-get"));
    }

    #[test]
    fn scheme_builtin_list_contains_console_helpers() {
        assert!(SCHEME_BUILTINS.contains(&"console-list"));
        assert!(SCHEME_BUILTINS.contains(&"console-clear"));
    }

    #[test]
    fn scheme_builtin_list_contains_inspect_helpers() {
        assert!(SCHEME_BUILTINS.contains(&"inspect-info"));
        assert!(SCHEME_BUILTINS.contains(&"inspect-highlight"));
        assert!(SCHEME_BUILTINS.contains(&"inspect-hide-highlight"));
        assert!(SCHEME_BUILTINS.contains(&"inspect-pick"));
    }

    #[test]
    fn scheme_builtin_list_contains_tracing_helpers() {
        assert!(SCHEME_BUILTINS.contains(&"tracing-start"));
        assert!(SCHEME_BUILTINS.contains(&"tracing-stop"));
    }

    fn expect_test_string(value: &Value) -> String {
        match value {
            Value::String(text) => text.to_plain_string(),
            other => panic!("expected string, got {other}"),
        }
    }
}
