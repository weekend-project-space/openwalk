use chromiumoxide::cdp::{
    browser_protocol::log::{
        ClearParams as LogClearParams, EnableParams as LogEnableParams, EventEntryAdded, LogEntry,
    },
    js_protocol::runtime::{
        ConsoleApiCalledType, DiscardConsoleEntriesParams, EnableParams as RuntimeEnableParams,
        EventConsoleApiCalled, EventExceptionThrown, RemoteObject, StackTrace,
    },
};

use super::super::{
    actor::BrowserActor,
    types::{BrowserValue, ConsoleEntry, ConsoleState},
    util::serialize_to_browser_value,
    *,
};

impl BrowserActor {
    pub(in crate::browser) async fn ensure_console_tracking_for_page(
        &mut self,
        page: Page,
    ) -> Result<()> {
        let page_id = page.target_id().as_ref().to_string();
        if self.observed_console_targets.contains(page_id.as_str()) {
            return Ok(());
        }

        self.install_console_buffer_for_page(&page).await?;

        page.execute(RuntimeEnableParams::default())
            .await
            .with_context(|| format!("failed to enable runtime tracking for page `{page_id}`"))?;
        page.execute(LogEnableParams::default())
            .await
            .with_context(|| format!("failed to enable log tracking for page `{page_id}`"))?;

        let mut console_events = page
            .event_listener::<EventConsoleApiCalled>()
            .await
            .with_context(|| {
                format!("failed to subscribe to console events for page `{page_id}`")
            })?;
        let mut exception_events = page
            .event_listener::<EventExceptionThrown>()
            .await
            .with_context(|| {
                format!("failed to subscribe to exception events for page `{page_id}`")
            })?;
        let mut log_events = page
            .event_listener::<EventEntryAdded>()
            .await
            .with_context(|| format!("failed to subscribe to log events for page `{page_id}`"))?;

        let console_state = self.console_state.clone();
        let observed_page_id = page_id.clone();
        let task = tokio::spawn(async move {
            let mut console_closed = false;
            let mut exception_closed = false;
            let mut log_closed = false;

            while !(console_closed && exception_closed && log_closed) {
                tokio::select! {
                    event = console_events.next(), if !console_closed => {
                        match event {
                            Some(event) => {
                                if let Ok(mut state) = console_state.lock() {
                                    state.push(console_entry_from_api_called(
                                        observed_page_id.as_str(),
                                        event.as_ref().clone(),
                                    ));
                                }
                            }
                            None => console_closed = true,
                        }
                    }
                    event = exception_events.next(), if !exception_closed => {
                        match event {
                            Some(event) => {
                                if let Ok(mut state) = console_state.lock() {
                                    state.push(console_entry_from_exception(
                                        observed_page_id.as_str(),
                                        event.as_ref().clone(),
                                    ));
                                }
                            }
                            None => exception_closed = true,
                        }
                    }
                    event = log_events.next(), if !log_closed => {
                        match event {
                            Some(event) => {
                                if let Ok(mut state) = console_state.lock() {
                                    state.push(console_entry_from_log(
                                        observed_page_id.as_str(),
                                        event.as_ref().clone().entry,
                                    ));
                                }
                            }
                            None => log_closed = true,
                        }
                    }
                }
            }
        });

        self.observed_console_targets.insert(page_id);
        self.console_listener_tasks.push(task);
        Ok(())
    }

    pub(in crate::browser) async fn console(
        &mut self,
        min_level: Option<String>,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        self.ensure_console_tracking_for_page(page.clone()).await?;
        tokio::time::sleep(Duration::from_millis(60)).await;
        let page_id = page.target_id().as_ref().to_string();
        let buffered_entries = self
            .load_console_buffer_entries(&page, page_id.as_str())
            .await
            .unwrap_or_else(|_| self.console_page_entries(page_id.as_str()).unwrap_or_default());
        let tracked_entries = self.console_page_entries(page_id.as_str()).unwrap_or_default();
        let entries = merge_console_entries(buffered_entries, tracked_entries);
        let base_timestamp = self
            .page_time_origin_seconds(&page)
            .await
            .ok()
            .or_else(|| entries.first().map(|entry| entry.timestamp));
        let entries = filter_console_entries(entries, min_level.as_deref())?;
        let lines = format_console_entries(entries.as_slice(), base_timestamp);
        serialize_to_browser_value(&lines, "failed to serialize console entries")
    }

    pub(in crate::browser) async fn console_clear(&mut self) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        self.ensure_console_tracking_for_page(page.clone()).await?;
        let page_id = page.target_id().as_ref().to_string();

        page.execute(DiscardConsoleEntriesParams::default())
            .await
            .context("failed to clear runtime console entries")?;
        page.execute(LogClearParams::default())
            .await
            .context("failed to clear browser log entries")?;
        let _ = page.evaluate(CONSOLE_BUFFER_CLEAR_EVAL_JS).await;

        let mut state = self
            .console_state
            .lock()
            .map_err(|_| anyhow!("console log is not available"))?;
        state.clear_page(page_id.as_str());

        Ok(BrowserValue::Boolean(true))
    }

    pub(in crate::browser) fn clear_console_page_state(&mut self, page_id: &str) {
        self.observed_console_targets.remove(page_id);
        if let Ok(mut state) = self.console_state.lock() {
            state.clear_page(page_id);
        }
    }

    fn console_page_entries(&self, page_id: &str) -> Result<Vec<ConsoleEntry>> {
        let state = self
            .console_state
            .lock()
            .map_err(|_| anyhow!("console log is not available"))?;
        Ok(state.page_entries(page_id))
    }

    async fn install_console_buffer_for_page(&self, page: &Page) -> Result<()> {
        page.add_init_script(CONSOLE_BUFFER_INIT_SCRIPT)
            .await
            .context("failed to install console init script")?;
        let _ = page.evaluate(CONSOLE_BUFFER_INSTALL_EVAL_JS).await;
        Ok(())
    }

    async fn load_console_buffer_entries(
        &self,
        page: &Page,
        page_id: &str,
    ) -> Result<Vec<ConsoleEntry>> {
        let mut value: serde_json::Value = page
            .evaluate(CONSOLE_BUFFER_READ_EVAL_JS)
            .await
            .context("failed to read page console buffer")?
            .into_value()
            .context("page console buffer returned a non-serializable value")?;

        let Some(items) = value.as_array_mut() else {
            return Ok(Vec::new());
        };

        for (index, item) in items.iter_mut().enumerate() {
            if let Some(object) = item.as_object_mut() {
                object.insert(
                    "sequence".to_string(),
                    serde_json::Value::Number((index as u64).into()),
                );
                object.insert(
                    "page_id".to_string(),
                    serde_json::Value::String(page_id.to_string()),
                );
            }
        }

        serde_json::from_value(value).context("failed to decode page console buffer")
    }

    async fn page_time_origin_seconds(&self, page: &Page) -> Result<f64> {
        let value: serde_json::Value = page
            .evaluate("() => (typeof performance?.timeOrigin === 'number' ? performance.timeOrigin : Date.now()) / 1000")
            .await
            .context("failed to read page time origin")?
            .into_value()
            .context("page time origin returned a non-serializable value")?;
        value
            .as_f64()
            .ok_or_else(|| anyhow!("page time origin did not return a number"))
    }
}

impl ConsoleState {
    fn push(&mut self, mut entry: ConsoleEntry) {
        entry.sequence = self.next_sequence;
        self.next_sequence += 1;
        self.entries.push(entry);
    }

    fn page_entries(&self, page_id: &str) -> Vec<ConsoleEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.page_id == page_id)
            .cloned()
            .collect()
    }

    fn clear_page(&mut self, page_id: &str) {
        self.entries.retain(|entry| entry.page_id != page_id);
    }
}

fn console_entry_from_api_called(page_id: &str, event: EventConsoleApiCalled) -> ConsoleEntry {
    let args = remote_objects_to_text(&event.args);
    let text = if args.is_empty() {
        event.r#type.as_ref().to_string()
    } else {
        args.join(" ")
    };
    let (url, line_number, column_number) = stack_trace_location(event.stack_trace.as_ref());

    ConsoleEntry {
        sequence: 0,
        page_id: page_id.to_string(),
        kind: "console".to_string(),
        level: console_api_level(&event.r#type).to_string(),
        text,
        args,
        event_type: Some(event.r#type.as_ref().to_string()),
        source: Some("runtime.console".to_string()),
        url,
        line_number,
        column_number,
        context: event.context,
        timestamp: *event.timestamp.inner(),
    }
}

fn console_entry_from_exception(page_id: &str, event: EventExceptionThrown) -> ConsoleEntry {
    let details = event.exception_details;
    let args = details
        .exception
        .as_ref()
        .map(remote_object_to_text)
        .into_iter()
        .collect::<Vec<_>>();
    let text = args
        .first()
        .cloned()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| details.text.clone());
    let (stack_url, stack_line_number, stack_column_number) =
        stack_trace_location(details.stack_trace.as_ref());

    ConsoleEntry {
        sequence: 0,
        page_id: page_id.to_string(),
        kind: "exception".to_string(),
        level: "error".to_string(),
        text,
        args,
        event_type: Some("exception-thrown".to_string()),
        source: Some("runtime.exception".to_string()),
        url: details.url.or(stack_url),
        line_number: Some(details.line_number).or(stack_line_number),
        column_number: Some(details.column_number).or(stack_column_number),
        context: None,
        timestamp: *event.timestamp.inner(),
    }
}

fn console_entry_from_log(page_id: &str, entry: LogEntry) -> ConsoleEntry {
    let args = entry
        .args
        .as_deref()
        .map(remote_objects_to_text)
        .unwrap_or_default();
    let (stack_url, stack_line_number, stack_column_number) =
        stack_trace_location(entry.stack_trace.as_ref());
    let text = join_console_text(entry.text.as_str(), &args);

    ConsoleEntry {
        sequence: 0,
        page_id: page_id.to_string(),
        kind: "log".to_string(),
        level: entry.level.as_ref().to_string(),
        text,
        args,
        event_type: None,
        source: Some(entry.source.as_ref().to_string()),
        url: entry.url.or(stack_url),
        line_number: entry.line_number.or(stack_line_number),
        column_number: stack_column_number,
        context: None,
        timestamp: *entry.timestamp.inner(),
    }
}

fn console_api_level(kind: &ConsoleApiCalledType) -> &'static str {
    match kind {
        ConsoleApiCalledType::Error | ConsoleApiCalledType::Assert => "error",
        ConsoleApiCalledType::Warning => "warning",
        ConsoleApiCalledType::Info => "info",
        ConsoleApiCalledType::Debug => "debug",
        _ => "log",
    }
}

fn stack_trace_location(
    stack_trace: Option<&StackTrace>,
) -> (Option<String>, Option<i64>, Option<i64>) {
    let Some(frame) = stack_trace.and_then(|stack_trace| stack_trace.call_frames.first()) else {
        return (None, None, None);
    };

    (
        Some(frame.url.clone()),
        Some(frame.line_number),
        Some(frame.column_number),
    )
}

fn join_console_text(text: &str, args: &[String]) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return args.join(" ");
    }

    if args.is_empty() {
        return trimmed.to_string();
    }

    let joined_args = args.join(" ");
    if trimmed == joined_args {
        trimmed.to_string()
    } else {
        format!("{trimmed} {joined_args}")
    }
}

fn remote_objects_to_text(args: &[RemoteObject]) -> Vec<String> {
    args.iter().map(remote_object_to_text).collect()
}

fn remote_object_to_text(value: &RemoteObject) -> String {
    if let Some(raw) = value.value.as_ref() {
        return match raw {
            serde_json::Value::String(text) => text.clone(),
            _ => raw.to_string(),
        };
    }

    if let Some(raw) = value.unserializable_value.as_ref() {
        return raw.as_ref().to_string();
    }

    if let Some(description) = value.description.as_ref() {
        return description.clone();
    }

    if let Some(subtype) = value.subtype.as_ref() {
        return subtype.as_ref().to_string();
    }

    if let Some(class_name) = value.class_name.as_ref() {
        return class_name.clone();
    }

    value.r#type.as_ref().to_string()
}

fn filter_console_entries(
    entries: Vec<ConsoleEntry>,
    min_level: Option<&str>,
) -> Result<Vec<ConsoleEntry>> {
    let Some(min_level) = min_level else {
        return Ok(entries);
    };

    let threshold = console_level_rank(min_level).ok_or_else(|| {
        anyhow!(
            "unsupported console min-level `{min_level}`. Use one of: log, debug, info, warn, warning, error"
        )
    })?;

    Ok(entries
        .into_iter()
        .filter(|entry| {
            console_level_rank(entry.level.as_str())
                .map(|rank| rank >= threshold)
                .unwrap_or(false)
        })
        .collect())
}

fn console_level_rank(level: &str) -> Option<u8> {
    match level.trim().to_ascii_lowercase().as_str() {
        "trace" | "verbose" | "log" | "debug" => Some(0),
        "info" => Some(1),
        "warn" | "warning" => Some(2),
        "error" => Some(3),
        _ => None,
    }
}

fn merge_console_entries(
    mut buffered_entries: Vec<ConsoleEntry>,
    tracked_entries: Vec<ConsoleEntry>,
) -> Vec<ConsoleEntry> {
    if buffered_entries.is_empty() {
        let mut entries = tracked_entries;
        sort_console_entries(&mut entries);
        return entries;
    }

    buffered_entries.extend(
        tracked_entries
            .into_iter()
            .filter(|entry| entry.kind == "log"),
    );
    sort_console_entries(&mut buffered_entries);
    buffered_entries
}

fn sort_console_entries(entries: &mut [ConsoleEntry]) {
    entries.sort_by(|left, right| {
        left.timestamp
            .partial_cmp(&right.timestamp)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.sequence.cmp(&right.sequence))
    });
}

fn format_console_entries(entries: &[ConsoleEntry], base_timestamp: Option<f64>) -> Vec<String> {
    entries
        .iter()
        .map(|entry| format_console_entry(entry, base_timestamp))
        .collect()
}

fn format_console_entry(entry: &ConsoleEntry, base_timestamp: Option<f64>) -> String {
    let elapsed_ms = base_timestamp
        .map(|base| ((entry.timestamp - base).max(0.0) * 1000.0).round() as u64)
        .unwrap_or(0);
    let prefix = format!(
        "[{:>8}ms] [{}]",
        elapsed_ms,
        console_level_label(entry.level.as_str())
    );
    let location = console_entry_location(entry);

    if let Some(location) = location {
        if entry.text.contains('\n') {
            format!("{prefix} {}\n @ {location}", entry.text)
        } else {
            format!("{prefix} {} @ {location}", entry.text)
        }
    } else {
        format!("{prefix} {}", entry.text)
    }
}

fn console_level_label(level: &str) -> &'static str {
    match level.trim().to_ascii_lowercase().as_str() {
        "warning" | "warn" => "WARNING",
        "error" => "ERROR",
        "info" => "INFO",
        "debug" => "DEBUG",
        _ => "LOG",
    }
}

fn console_entry_location(entry: &ConsoleEntry) -> Option<String> {
    let url = entry.url.as_deref()?.trim();
    if url.is_empty() {
        return None;
    }

    match (entry.line_number, entry.column_number) {
        (Some(line), Some(column)) => Some(format!("{url}:{line}:{column}")),
        (Some(line), None) => Some(format!("{url}:{line}")),
        _ => Some(url.to_string()),
    }
}

const CONSOLE_BUFFER_INIT_SCRIPT: &str = r#"
(() => {
  if (window.__openwalkConsoleInstalled) {
    return;
  }
  window.__openwalkConsoleInstalled = true;
  window.__openwalkConsoleEntries = window.__openwalkConsoleEntries || [];
  const maxEntries = 500;
  const toText = (value) => {
    if (typeof value === "string") {
      return value;
    }
    if (value instanceof Error) {
      return value.stack || value.message || String(value);
    }
    try {
      const encoded = JSON.stringify(value);
      if (typeof encoded === "string") {
        return encoded;
      }
    } catch (_) {}
    return String(value);
  };
  const pushEntry = (entry) => {
    try {
      const list = window.__openwalkConsoleEntries || (window.__openwalkConsoleEntries = []);
      list.push(entry);
      if (list.length > maxEntries) {
        list.splice(0, list.length - maxEntries);
      }
    } catch (_) {}
  };
  const levelForMethod = (method) => {
    switch (method) {
      case "warn":
        return "warning";
      case "error":
        return "error";
      case "info":
        return "info";
      case "debug":
        return "debug";
      default:
        return "log";
    }
  };
  const normalizeUrl = (value) => {
    if (typeof value !== "string" || value.length === 0 || value === "<anonymous>" || value === "anonymous") {
      return location.href;
    }
    return value;
  };
  const parseStackLocation = (stack) => {
    const fallback = {
      url: location.href,
      line_number: null,
      column_number: null
    };
    if (typeof stack !== "string") {
      return fallback;
    }
    const frames = stack
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .slice(1)
      .map((line) => {
        const match =
          line.match(/^at .* \((.*):(\d+):(\d+)\)$/) ||
          line.match(/^at (.*):(\d+):(\d+)$/) ||
          line.match(/^(.*):(\d+):(\d+)$/);
        if (!match) {
          return null;
        }
        return {
          raw: line,
          url: normalizeUrl(match[1]),
          line_number: Number(match[2]),
          column_number: Number(match[3])
        };
      })
      .filter(Boolean);
    const preferred = frames.find(
      (frame) =>
        !/\bcaptureConsoleLocation\b/.test(frame.raw) &&
        !/\bparseStackLocation\b/.test(frame.raw) &&
        !/\bconsole\./.test(frame.raw) &&
        !/\[as (log|info|warn|error|debug)\]/.test(frame.raw)
    );
    if (preferred) {
      return {
        url: preferred.url,
        line_number: preferred.line_number,
        column_number: preferred.column_number
      };
    }
    if (frames.length > 0) {
      return {
        url: frames[0].url,
        line_number: frames[0].line_number,
        column_number: frames[0].column_number
      };
    }
    return fallback;
  };
  const captureConsoleLocation = () => {
    try {
      throw new Error();
    } catch (error) {
      return parseStackLocation(error && error.stack);
    }
  };
  for (const method of ["log", "info", "warn", "error", "debug"]) {
    const original = typeof console[method] === "function" ? console[method].bind(console) : null;
    if (!original) {
      continue;
    }
    console[method] = (...args) => {
      const textArgs = args.map(toText);
      const stackLocation = captureConsoleLocation();
      pushEntry({
        kind: "console",
        level: levelForMethod(method),
        text: textArgs.join(" "),
        args: textArgs,
        event_type: method,
        source: "runtime.console",
        url: stackLocation.url,
        line_number: stackLocation.line_number,
        column_number: stackLocation.column_number,
        context: null,
        timestamp: Date.now() / 1000
      });
      return original(...args);
    };
  }
  window.addEventListener(
    "error",
    (event) => {
      const text = event.error
        ? toText(event.error)
        : (event.message || "Unhandled error");
      const stackLocation = parseStackLocation(event.error && event.error.stack);
      pushEntry({
        kind: "exception",
        level: "error",
        text,
        args: [text],
        event_type: "exception-thrown",
        source: "runtime.exception",
        url: stackLocation.url || event.filename || location.href,
        line_number: stackLocation.line_number ?? (typeof event.lineno === "number" ? event.lineno : null),
        column_number: stackLocation.column_number ?? (typeof event.colno === "number" ? event.colno : null),
        context: null,
        timestamp: Date.now() / 1000
      });
    },
    true
  );
  window.addEventListener(
    "unhandledrejection",
    (event) => {
      const text = toText(event.reason);
      const stackLocation = parseStackLocation(event.reason && event.reason.stack);
      pushEntry({
        kind: "exception",
        level: "error",
        text,
        args: [text],
        event_type: "unhandledrejection",
        source: "runtime.exception",
        url: stackLocation.url,
        line_number: stackLocation.line_number,
        column_number: stackLocation.column_number,
        context: null,
        timestamp: Date.now() / 1000
      });
    },
    true
  );
})();
"#;

const CONSOLE_BUFFER_INSTALL_EVAL_JS: &str = r#"() => {
    if (window.__openwalkConsoleInstalled) {
        return true;
    }
    window.__openwalkConsoleInstalled = true;
    window.__openwalkConsoleEntries = window.__openwalkConsoleEntries || [];
    const maxEntries = 500;
    const toText = (value) => {
        if (typeof value === "string") {
            return value;
        }
        if (value instanceof Error) {
            return value.stack || value.message || String(value);
        }
        try {
            const encoded = JSON.stringify(value);
            if (typeof encoded === "string") {
                return encoded;
            }
        } catch (_) {}
        return String(value);
    };
    const pushEntry = (entry) => {
        try {
            const list = window.__openwalkConsoleEntries || (window.__openwalkConsoleEntries = []);
            list.push(entry);
            if (list.length > maxEntries) {
                list.splice(0, list.length - maxEntries);
            }
        } catch (_) {}
    };
    const levelForMethod = (method) => {
        switch (method) {
            case "warn":
                return "warning";
            case "error":
                return "error";
            case "info":
                return "info";
            case "debug":
                return "debug";
            default:
                return "log";
        }
    };
    const normalizeUrl = (value) => {
        if (typeof value !== "string" || value.length === 0 || value === "<anonymous>" || value === "anonymous") {
            return location.href;
        }
        return value;
    };
    const parseStackLocation = (stack) => {
        const fallback = {
            url: location.href,
            line_number: null,
            column_number: null
        };
        if (typeof stack !== "string") {
            return fallback;
        }
        const frames = stack
            .split("\n")
            .map((line) => line.trim())
            .filter(Boolean)
            .slice(1)
            .map((line) => {
                const match =
                    line.match(/^at .* \((.*):(\d+):(\d+)\)$/) ||
                    line.match(/^at (.*):(\d+):(\d+)$/) ||
                    line.match(/^(.*):(\d+):(\d+)$/);
                if (!match) {
                    return null;
                }
                return {
                    raw: line,
                    url: normalizeUrl(match[1]),
                    line_number: Number(match[2]),
                    column_number: Number(match[3])
                };
            })
            .filter(Boolean);
        const preferred = frames.find(
            (frame) =>
                !/\bcaptureConsoleLocation\b/.test(frame.raw) &&
                !/\bparseStackLocation\b/.test(frame.raw) &&
                !/\bconsole\./.test(frame.raw) &&
                !/\[as (log|info|warn|error|debug)\]/.test(frame.raw)
        );
        if (preferred) {
            return {
                url: preferred.url,
                line_number: preferred.line_number,
                column_number: preferred.column_number
            };
        }
        if (frames.length > 0) {
            return {
                url: frames[0].url,
                line_number: frames[0].line_number,
                column_number: frames[0].column_number
            };
        }
        return fallback;
    };
    const captureConsoleLocation = () => {
        try {
            throw new Error();
        } catch (error) {
            return parseStackLocation(error && error.stack);
        }
    };
    for (const method of ["log", "info", "warn", "error", "debug"]) {
        const original = typeof console[method] === "function" ? console[method].bind(console) : null;
        if (!original) {
            continue;
        }
        console[method] = (...args) => {
            const textArgs = args.map(toText);
            const stackLocation = captureConsoleLocation();
            pushEntry({
                kind: "console",
                level: levelForMethod(method),
                text: textArgs.join(" "),
                args: textArgs,
                event_type: method,
                source: "runtime.console",
                url: stackLocation.url,
                line_number: stackLocation.line_number,
                column_number: stackLocation.column_number,
                context: null,
                timestamp: Date.now() / 1000
            });
            return original(...args);
        };
    }
    window.addEventListener(
        "error",
        (event) => {
            const text = event.error
                ? toText(event.error)
                : (event.message || "Unhandled error");
            const stackLocation = parseStackLocation(event.error && event.error.stack);
            pushEntry({
                kind: "exception",
                level: "error",
                text,
                args: [text],
                event_type: "exception-thrown",
                source: "runtime.exception",
                url: stackLocation.url || event.filename || location.href,
                line_number: stackLocation.line_number ?? (typeof event.lineno === "number" ? event.lineno : null),
                column_number: stackLocation.column_number ?? (typeof event.colno === "number" ? event.colno : null),
                context: null,
                timestamp: Date.now() / 1000
            });
        },
        true
    );
    window.addEventListener(
        "unhandledrejection",
        (event) => {
            const text = toText(event.reason);
            const stackLocation = parseStackLocation(event.reason && event.reason.stack);
            pushEntry({
                kind: "exception",
                level: "error",
                text,
                args: [text],
                event_type: "unhandledrejection",
                source: "runtime.exception",
                url: stackLocation.url,
                line_number: stackLocation.line_number,
                column_number: stackLocation.column_number,
                context: null,
                timestamp: Date.now() / 1000
            });
        },
        true
    );
    return true;
}"#;

const CONSOLE_BUFFER_READ_EVAL_JS: &str =
    r#"() => Array.isArray(window.__openwalkConsoleEntries) ? window.__openwalkConsoleEntries : []"#;

const CONSOLE_BUFFER_CLEAR_EVAL_JS: &str = r#"() => {
    window.__openwalkConsoleEntries = [];
    return true;
}"#;

#[cfg(test)]
mod tests {
    use chromiumoxide::cdp::{
        browser_protocol::log::{LogEntry, LogEntryLevel, LogEntrySource},
        js_protocol::runtime::{
            ConsoleApiCalledType, EventConsoleApiCalled, EventExceptionThrown, ExceptionDetails,
            ExecutionContextId, RemoteObject, RemoteObjectType, Timestamp,
        },
    };

    use super::*;

    #[test]
    fn remote_object_to_text_prefers_json_value() {
        let value = RemoteObject::builder()
            .r#type(RemoteObjectType::String)
            .value("hello")
            .description("ignored")
            .build()
            .expect("remote object should build");

        assert_eq!(remote_object_to_text(&value), "hello");
    }

    #[test]
    fn remote_object_to_text_falls_back_to_description() {
        let value = RemoteObject::builder()
            .r#type(RemoteObjectType::Object)
            .description("Object(foo)")
            .build()
            .expect("remote object should build");

        assert_eq!(remote_object_to_text(&value), "Object(foo)");
    }

    #[test]
    fn console_state_clear_page_removes_entries() {
        let mut state = ConsoleState::default();
        state.push(ConsoleEntry {
            sequence: 0,
            page_id: "page-a".to_string(),
            kind: "console".to_string(),
            level: "info".to_string(),
            text: "ready".to_string(),
            args: vec!["ready".to_string()],
            event_type: Some("info".to_string()),
            source: Some("runtime.console".to_string()),
            url: None,
            line_number: None,
            column_number: None,
            context: None,
            timestamp: 1.0,
        });
        state.push(ConsoleEntry {
            sequence: 0,
            page_id: "page-a".to_string(),
            kind: "exception".to_string(),
            level: "error".to_string(),
            text: "boom".to_string(),
            args: vec!["boom".to_string()],
            event_type: Some("exception-thrown".to_string()),
            source: Some("runtime.exception".to_string()),
            url: None,
            line_number: None,
            column_number: None,
            context: None,
            timestamp: 2.0,
        });

        state.clear_page("page-a");

        assert!(state.page_entries("page-a").is_empty());
    }

    #[test]
    fn console_entry_from_api_called_joins_args() {
        let event = EventConsoleApiCalled {
            r#type: ConsoleApiCalledType::Log,
            args: vec![
                RemoteObject::builder()
                    .r#type(RemoteObjectType::String)
                    .value("hello")
                    .build()
                    .expect("remote object should build"),
                RemoteObject::builder()
                    .r#type(RemoteObjectType::Number)
                    .value(42)
                    .build()
                    .expect("remote object should build"),
            ],
            execution_context_id: ExecutionContextId::new(1),
            timestamp: Timestamp::new(10.0),
            stack_trace: None,
            context: None,
        };

        let entry = console_entry_from_api_called("page-a", event);

        assert_eq!(entry.text, "hello 42");
        assert_eq!(entry.level, "log");
    }

    #[test]
    fn console_entry_from_exception_uses_exception_value() {
        let event = EventExceptionThrown {
            timestamp: Timestamp::new(11.0),
            exception_details: ExceptionDetails {
                exception_id: 1,
                text: "Uncaught".to_string(),
                line_number: 7,
                column_number: 3,
                script_id: None,
                url: Some("https://example.com/app.js".to_string()),
                stack_trace: None,
                exception: Some(
                    RemoteObject::builder()
                        .r#type(RemoteObjectType::Object)
                        .description("ReferenceError: missingValue is not defined")
                        .build()
                        .expect("remote object should build"),
                ),
                execution_context_id: None,
                exception_meta_data: None,
            },
        };

        let entry = console_entry_from_exception("page-a", event);

        assert_eq!(entry.kind, "exception");
        assert_eq!(entry.level, "error");
        assert!(entry.text.contains("ReferenceError"));
    }

    #[test]
    fn console_entry_from_log_combines_text_and_args() {
        let entry = console_entry_from_log(
            "page-a",
            LogEntry::builder()
                .source(LogEntrySource::Javascript)
                .level(LogEntryLevel::Warning)
                .text("warn")
                .timestamp(chromiumoxide::cdp::js_protocol::runtime::Timestamp::new(
                    12.0,
                ))
                .arg(
                    RemoteObject::builder()
                        .r#type(RemoteObjectType::String)
                        .value("details")
                        .build()
                        .expect("remote object should build"),
                )
                .build()
                .expect("log entry should build"),
        );

        assert_eq!(entry.level, "warning");
        assert_eq!(entry.text, "warn details");
    }

    #[test]
    fn filter_console_entries_applies_min_level_threshold() {
        let entries = vec![
            ConsoleEntry {
                sequence: 0,
                page_id: "page-a".to_string(),
                kind: "console".to_string(),
                level: "log".to_string(),
                text: "ready".to_string(),
                args: vec!["ready".to_string()],
                event_type: Some("log".to_string()),
                source: Some("runtime.console".to_string()),
                url: None,
                line_number: None,
                column_number: None,
                context: None,
                timestamp: 1.0,
            },
            ConsoleEntry {
                sequence: 1,
                page_id: "page-a".to_string(),
                kind: "exception".to_string(),
                level: "error".to_string(),
                text: "boom".to_string(),
                args: vec!["boom".to_string()],
                event_type: Some("exception-thrown".to_string()),
                source: Some("runtime.exception".to_string()),
                url: None,
                line_number: None,
                column_number: None,
                context: None,
                timestamp: 2.0,
            },
        ];

        let filtered =
            filter_console_entries(entries, Some("warn")).expect("console filtering should work");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].level, "error");
    }

    #[test]
    fn filter_console_entries_rejects_unknown_min_level() {
        let error = filter_console_entries(Vec::new(), Some("fatal"))
            .expect_err("unknown console min-level should fail");

        assert!(error.to_string().contains("unsupported console min-level"));
    }

    #[test]
    fn format_console_entry_matches_log_style() {
        let entry = ConsoleEntry {
            sequence: 1,
            page_id: "page-a".to_string(),
            kind: "console".to_string(),
            level: "warning".to_string(),
            text: "warn details".to_string(),
            args: vec!["warn".to_string(), "details".to_string()],
            event_type: Some("warn".to_string()),
            source: Some("runtime.console".to_string()),
            url: Some("https://example.com/app.js".to_string()),
            line_number: Some(42),
            column_number: Some(7),
            context: None,
            timestamp: 2.5,
        };

        let line = format_console_entry(&entry, Some(1.0));

        assert_eq!(
            line,
            "[    1500ms] [WARNING] warn details @ https://example.com/app.js:42:7"
        );
    }
}
