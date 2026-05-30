use crate::tool_metadata::ToolArgument;

pub(super) fn tool_usage_from_metadata(name: &str, args: &[ToolArgument]) -> String {
    if let Some(usage) = builtin_tool_usage_override(name) {
        return usage.to_string();
    }

    let mut usage = String::from(name);
    for (index, arg) in args.iter().enumerate() {
        usage.push(' ');
        usage.push_str(render_tool_usage_arg(name, index, arg).as_str());
    }
    usage
}

fn builtin_tool_usage_override(name: &str) -> Option<&'static str> {
    match name {
        "element-double-click" => Some("element-double-click <selector>"),
        "element-right-click" => Some("element-right-click <selector>"),
        "element-type" => Some("element-type <selector> <text>"),
        "element-fill" => Some("element-fill <selector> <text>"),
        "keyboard-press" => Some("keyboard-press <key>"),
        "keyboard-type" => Some("keyboard-type <text>"),
        "keyboard-down" => Some("keyboard-down <key>"),
        "keyboard-up" => Some("keyboard-up <key>"),
        "element-select" => Some("element-select <selector> <value>"),
        "element-check" => Some("element-check <selector>"),
        "element-uncheck" => Some("element-uncheck <selector>"),
        "js-wait" => Some("js-wait <expression>"),
        "element-exists" => Some("element-exists <selector>"),
        "element-hover" => Some("element-hover <selector>"),
        "page-screenshot" => Some("page-screenshot <path>"),
        "element-screenshot" => Some("element-screenshot <selector> <path>"),
        "page-pdf" => Some("page-pdf <path>"),
        "page-scroll-to" => Some("page-scroll-to <x> <y>"),
        "page-scroll-by" => Some("page-scroll-by <x> <y>"),
        "browser-resize" => Some("browser-resize <width> <height>"),
        "network-wait-response" => Some("network-wait-response <url_contains>"),
        "inspect-info" => Some("inspect-info <selector>"),
        "inspect-highlight" => Some("inspect-highlight <selector>"),
        "inspect-pick" => Some("inspect-pick [timeout-ms]"),
        "tracing-start" => Some("tracing-start [categories]"),
        "tracing-stop" => Some("tracing-stop <path>"),
        "mouse-move" => Some("mouse-move <x> <y>"),
        "mouse-click" => Some("mouse-click <x> <y>"),
        "mouse-down" => Some("mouse-down <x> <y> <button>"),
        "mouse-up" => Some("mouse-up <x> <y> <button>"),
        "mouse-wheel" => Some("mouse-wheel <x> <y> <delta-x> <delta-y>"),
        "touch-tap" => Some("touch-tap <x> <y>"),
        _ => None,
    }
}

fn render_tool_usage_arg(tool_name: &str, index: usize, arg: &ToolArgument) -> String {
    let token = match (tool_name, index, arg.name.as_str()) {
        ("element-upload", 1, "file") => "file...".to_string(),
        _ => arg.name.clone(),
    };

    if arg.required {
        format!("<{token}>")
    } else {
        format!("[{token}]")
    }
}

pub(super) fn compact_tool_description(name: &str, description: &str) -> String {
    if !description.starts_with("OpenWalk 内置 ") {
        return trim_tool_description(description);
    }

    match name {
        "page-back" => "页面后退".to_string(),
        "page-forward" => "页面前进".to_string(),
        "page-reload" => "刷新当前页面".to_string(),
        "element-double-click" => "双击匹配元素".to_string(),
        "element-right-click" => "右键点击匹配元素".to_string(),
        "element-type" => "向可编辑元素输入文本".to_string(),
        "element-fill" => "填充输入框文本".to_string(),
        "keyboard-press" => "按下一个按键".to_string(),
        "keyboard-type" => "键入一段文本".to_string(),
        "keyboard-down" => "按下按键但不释放".to_string(),
        "keyboard-up" => "释放一个按键".to_string(),
        "element-select" => "选择下拉框选项".to_string(),
        "element-check" => "勾选匹配元素".to_string(),
        "element-uncheck" => "取消勾选匹配元素".to_string(),
        "js-wait" => "等待 JavaScript 条件成立".to_string(),
        "element-exists" => "检查元素是否存在".to_string(),
        "element-hover" => "悬停到匹配元素上".to_string(),
        "page-screenshot" => "保存当前页面截图".to_string(),
        "element-screenshot" => "保存元素截图".to_string(),
        "page-pdf" => "导出页面为 PDF".to_string(),
        "page-wait-navigation" => "等待页面导航完成".to_string(),
        "page-scroll-to" => "滚动到指定页面坐标".to_string(),
        "page-scroll-by" => "按偏移量滚动页面".to_string(),
        "browser-resize" => "调整浏览器窗口大小".to_string(),
        "browser-version" => "读取浏览器版本信息".to_string(),
        "performance-metrics" => "读取页面性能指标".to_string(),
        "network-wait-response" => "等待匹配的网络响应".to_string(),
        "console-clear" => "清空已记录的控制台日志".to_string(),
        "inspect-info" => "读取元素诊断信息".to_string(),
        "inspect-highlight" => "高亮匹配元素".to_string(),
        "inspect-hide-highlight" => "隐藏调试高亮".to_string(),
        "inspect-pick" => "交互式拾取页面元素".to_string(),
        "tracing-start" => "开始记录 tracing".to_string(),
        "tracing-stop" => "停止 tracing 并导出".to_string(),
        "mouse-move" => "移动鼠标到指定坐标".to_string(),
        "mouse-click" => "在指定坐标点击鼠标".to_string(),
        "mouse-down" => "按下鼠标按键".to_string(),
        "mouse-up" => "释放鼠标按键".to_string(),
        "mouse-wheel" => "滚动鼠标滚轮".to_string(),
        "touch-tap" => "模拟触摸点击".to_string(),
        _ => trim_tool_description(description),
    }
}

pub(super) fn trim_tool_description(description: &str) -> String {
    description
        .trim()
        .trim_end_matches('。')
        .trim_end_matches('.')
        .to_string()
}
