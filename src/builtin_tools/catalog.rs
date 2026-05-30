use crate::tool_metadata::{ToolArgument, ToolMetadata, ToolReturn};

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
    "element-upload",
    "element-drag",
    "page-snapshot",
    "page-screenshot",
    "element-screenshot",
    "page-pdf",
    "js-eval",
    "page-wait-navigation",
    "page-scroll-to",
    "page-scroll-by",
    "device-viewport",
    "tab-list",
    "tab-new",
    "tab-select",
    "tab-close",
    "browser-version",
    "performance-metrics",
    "network-list",
    "network-wait-response",
    "network-response-body",
    "console",
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

pub fn builtin_tool_metadata(name: &str) -> Option<ToolMetadata> {
    if !SCHEME_BUILTINS.contains(&name) {
        return None;
    }

    Some(match name {
        "browser-open" => ToolMetadata {
            name: name.to_string(),
            description: "打开浏览器并导航到指定 URL。".to_string(),
            args: vec![tool_arg("url", "string", true, "要打开的网页地址")],
            returns: ToolReturn {
                return_type: "string".to_string(),
                description: "页面标题；如果标题不可用则返回最终 URL。".to_string(),
            },
            examples: vec![
                "openwalk exec browser-open https://www.baidu.com".to_string(),
                "openwalk exec browser-open https://example.com -s=demo --headed --new-tab"
                    .to_string(),
            ],
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
        "element-upload" => ToolMetadata {
            name: name.to_string(),
            description: "向文件输入框上传一个或多个本地文件。".to_string(),
            args: vec![
                tool_arg("selector", "string", true, "文件输入框选择器"),
                tool_arg("file", "string", true, "要上传的本地文件路径，可重复传多个"),
            ],
            returns: ToolReturn {
                return_type: "array".to_string(),
                description: "成功设置到输入框中的绝对文件路径数组。".to_string(),
            },
            examples: vec![
                "openwalk exec element-upload \"input[type=file]\" ./avatar.png".to_string(),
                "openwalk exec element-upload \"input[type=file]\" ./a.txt ./b.txt".to_string(),
            ],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "dom".to_string(),
                "upload".to_string(),
            ],
        },
        "element-drag" => ToolMetadata {
            name: name.to_string(),
            description: "把一个元素拖拽到另一个元素上。".to_string(),
            args: vec![
                tool_arg("source", "string", true, "拖拽起点元素选择器"),
                tool_arg("target", "string", true, "拖拽目标元素选择器"),
            ],
            returns: ToolReturn {
                return_type: "boolean".to_string(),
                description: "拖拽事件链派发成功时返回 true。".to_string(),
            },
            examples: vec!["openwalk exec element-drag \"#card-1\" \"#drop-zone\"".to_string()],
            domains: Vec::new(),
            read_only: false,
            requires_login: false,
            tags: vec!["builtin".to_string(), "dom".to_string(), "drag".to_string()],
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
        "page-snapshot" => ToolMetadata {
            name: name.to_string(),
            description: "抓取当前页面的结构化快照，包含标题、文本预览和交互元素摘要。".to_string(),
            args: Vec::new(),
            returns: ToolReturn {
                return_type: "object".to_string(),
                description: "页面结构化快照对象。".to_string(),
            },
            examples: vec!["openwalk exec page-snapshot".to_string()],
            domains: Vec::new(),
            read_only: true,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "page".to_string(),
                "snapshot".to_string(),
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
        "console" => ToolMetadata {
            name: name.to_string(),
            description: "读取当前页面已记录的控制台日志；可选按最低级别过滤。".to_string(),
            args: vec![tool_arg(
                "min-level",
                "string",
                false,
                "最小日志级别，可选：log、debug、info、warn、warning、error",
            )],
            returns: ToolReturn {
                return_type: "array".to_string(),
                description: "按时间、级别、正文、位置格式化后的日志文本行数组。".to_string(),
            },
            examples: vec![
                "openwalk exec console".to_string(),
                "openwalk exec console warn".to_string(),
            ],
            domains: Vec::new(),
            read_only: true,
            requires_login: false,
            tags: vec![
                "builtin".to_string(),
                "console".to_string(),
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
    } else if name == "console" || name.starts_with("console-") {
        "console"
    } else if name.starts_with("inspect-") {
        "inspect"
    } else if name.starts_with("tracing-") {
        "tracing"
    } else if name.starts_with("device-") {
        "device"
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
