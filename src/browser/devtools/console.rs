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

    pub(in crate::browser) async fn console_list(&mut self) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        self.ensure_console_tracking_for_page(page.clone()).await?;
        let page_id = page.target_id().as_ref().to_string();
        let entries = self.console_page_entries(page_id.as_str())?;
        serialize_to_browser_value(&entries, "failed to serialize console entries")
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
}
