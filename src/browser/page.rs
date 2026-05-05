use super::{
    actor::BrowserActor,
    types::{BrowserValue, Locator},
    util::{browser_request_timeout, json_to_browser_value, locator_name},
    *,
};

impl BrowserActor {
    pub(super) async fn goto(&mut self, url: String) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        if page_uses_browser_internal_url(&page).await? {
            bail!("cannot `page-goto` from an internal browser page; call `browser-open` instead");
        }

        let final_url = navigate_page_to_url(&page, url.as_str()).await?;
        Ok(BrowserValue::String(final_url))
    }

    pub(super) async fn back(&mut self) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let final_url = run_navigation_script_and_wait_for_dom_content_loaded(
            &page,
            "() => { history.back(); return true; }",
            "failed to navigate back",
            None,
        )
        .await?;
        Ok(BrowserValue::String(final_url))
    }

    pub(super) async fn forward(&mut self) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let final_url = run_navigation_script_and_wait_for_dom_content_loaded(
            &page,
            "() => { history.forward(); return true; }",
            "failed to navigate forward",
            None,
        )
        .await?;
        Ok(BrowserValue::String(final_url))
    }

    pub(super) async fn reload(&mut self) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let fallback_url = page.url().await?.unwrap_or_default();
        let final_url = run_navigation_script_and_wait_for_dom_content_loaded(
            &page,
            "() => { window.location.reload(); return true; }",
            "failed to reload the page",
            Some(fallback_url.as_str()),
        )
        .await?;
        Ok(BrowserValue::String(final_url))
    }

    pub(super) async fn wait_navigation(&mut self) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        page.wait_for_navigation()
            .await
            .context("failed while waiting for navigation")?;
        Ok(BrowserValue::String(page.url().await?.unwrap_or_default()))
    }

    pub(super) async fn page_screenshot(&mut self, path: String) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.save_screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .full_page(true)
                .build(),
            path.as_str(),
        )
        .await
        .with_context(|| format!("failed to save screenshot to `{path}`"))?;
        Ok(BrowserValue::String(path))
    }

    pub(super) async fn element_screenshot_locator(
        &mut self,
        locator: Locator,
        path: String,
    ) -> Result<BrowserValue> {
        let element = self.find_locator(&locator).await?;
        element
            .save_screenshot(CaptureScreenshotFormat::Png, path.as_str())
            .await
            .with_context(|| {
                format!(
                    "failed to save screenshot for {} `{}` to `{path}`",
                    locator_name(locator.kind()),
                    locator.raw()
                )
            })?;
        Ok(BrowserValue::String(path))
    }

    pub(super) async fn page_pdf(&mut self, path: String) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.save_pdf(PrintToPdfParams::default(), path.as_str())
            .await
            .with_context(|| format!("failed to save pdf to `{path}`"))?;
        Ok(BrowserValue::String(path))
    }

    pub(super) async fn page_snapshot(&mut self) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let value: serde_json::Value = page
            .evaluate(PAGE_SNAPSHOT_JS)
            .await
            .context("failed to capture page snapshot")?
            .into_value()
            .context("page snapshot returned a non-serializable value")?;

        Ok(json_to_browser_value(value))
    }

    pub(super) async fn scroll_to(&mut self, x: i64, y: i64) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        page.evaluate(format!("() => window.scrollTo({}, {})", x, y))
            .await
            .context("failed to scroll page")?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn scroll_by(&mut self, x: i64, y: i64) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        page.evaluate(format!("() => window.scrollBy({}, {})", x, y))
            .await
            .context("failed to scroll page by delta")?;
        Ok(BrowserValue::Boolean(true))
    }
}

pub(super) async fn navigate_page_to_url(page: &Page, url: &str) -> Result<String> {
    let expression = format!("() => {{ window.location.href = {url:?}; return true; }}");
    let error_context = format!("failed to navigate current page to `{url}`");
    run_navigation_script_and_wait_for_dom_content_loaded(
        page,
        expression.as_str(),
        error_context.as_str(),
        Some(url),
    )
    .await
}

async fn run_navigation_script_and_wait_for_dom_content_loaded(
    page: &Page,
    expression: &str,
    error_context: &str,
    fallback_url: Option<&str>,
) -> Result<String> {
    let mut frame_navigated_events = page
        .event_listener::<EventFrameNavigated>()
        .await
        .context("failed to subscribe to frame navigation events")?;
    let mut lifecycle_events = page
        .event_listener::<EventLifecycleEvent>()
        .await
        .context("failed to subscribe to page lifecycle events")?;
    let mut same_document_events = page
        .event_listener::<EventNavigatedWithinDocument>()
        .await
        .context("failed to subscribe to same-document navigation events")?;

    if let Err(err) = page.evaluate(expression).await {
        if !is_navigation_context_interrupted(&err) {
            return Err(err).context(error_context.to_string());
        }
    }

    let committed =
        wait_for_navigation_commit(page, &mut frame_navigated_events, &mut same_document_events)
            .await
            .with_context(|| error_context.to_string())?;

    wait_for_dom_content_loaded_after_commit(page, &mut lifecycle_events, committed.frame_id())
        .await
        .with_context(|| error_context.to_string())?;

    let final_url = page
        .url()
        .await?
        .unwrap_or_else(|| committed.url().to_string());
    if final_url.is_empty() {
        Ok(fallback_url.unwrap_or_default().to_string())
    } else {
        Ok(final_url)
    }
}

async fn wait_for_navigation_commit(
    page: &Page,
    frame_navigated_events: &mut EventStream<EventFrameNavigated>,
    same_document_events: &mut EventStream<EventNavigatedWithinDocument>,
) -> Result<NavigationCommit> {
    let timeout = sleep(browser_request_timeout());
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => bail!("Request timed out."),
            event = frame_navigated_events.next() => {
                let Some(event) = event else {
                    bail!("frame navigation listener closed unexpectedly");
                };
                if event.frame.parent_id.is_none() {
                    return Ok(NavigationCommit::NewDocument {
                        frame_id: event.frame.id.clone(),
                        url: event.frame.url.clone(),
                    });
                }
            }
            event = same_document_events.next() => {
                let Some(event) = event else {
                    bail!("same-document navigation listener closed unexpectedly");
                };
                if event_targets_main_frame(page, &event.frame_id).await {
                    return Ok(NavigationCommit::SameDocument {
                        frame_id: event.frame_id.clone(),
                        url: event.url.clone(),
                    });
                }
            }
        }
    }
}

async fn wait_for_dom_content_loaded_after_commit(
    page: &Page,
    lifecycle_events: &mut EventStream<EventLifecycleEvent>,
    frame_id: &FrameId,
) -> Result<()> {
    let timeout = sleep(browser_request_timeout());
    tokio::pin!(timeout);

    loop {
        if main_frame_ready_state_is_interactive(page).await? {
            return Ok(());
        }

        let poll_delay = sleep(Duration::from_millis(50));
        tokio::pin!(poll_delay);

        tokio::select! {
            _ = &mut timeout => bail!("Request timed out."),
            _ = &mut poll_delay => {}
            event = lifecycle_events.next() => {
                let Some(event) = event else {
                    bail!("navigation lifecycle listener closed unexpectedly");
                };
                if event.frame_id == *frame_id && event.name == "DOMContentLoaded" {
                    return Ok(());
                }
            }
        }
    }
}

async fn event_targets_main_frame(page: &Page, frame_id: &FrameId) -> bool {
    match page.frame_parent(frame_id.clone()).await {
        Ok(parent_frame) => parent_frame.is_none(),
        Err(_) => false,
    }
}

async fn main_frame_ready_state_is_interactive(page: &Page) -> Result<bool> {
    match page.evaluate("document.readyState").await {
        Ok(state) => {
            let ready_state: String = state.into_value().unwrap_or_default();
            Ok(matches!(ready_state.as_str(), "interactive" | "complete"))
        }
        Err(err) if is_navigation_context_interrupted(&err) => Ok(false),
        Err(err) => Err(err.into()),
    }
}

enum NavigationCommit {
    NewDocument { frame_id: FrameId, url: String },
    SameDocument { frame_id: FrameId, url: String },
}

impl NavigationCommit {
    fn frame_id(&self) -> &FrameId {
        match self {
            NavigationCommit::NewDocument { frame_id, .. } => frame_id,
            NavigationCommit::SameDocument { frame_id, .. } => frame_id,
        }
    }

    fn url(&self) -> &str {
        match self {
            NavigationCommit::NewDocument { url, .. } => url.as_str(),
            NavigationCommit::SameDocument { url, .. } => url.as_str(),
        }
    }
}

fn is_navigation_context_interrupted(err: &impl std::fmt::Display) -> bool {
    let message = err.to_string();
    message.contains("Execution context was destroyed")
        || message.contains("Cannot find context with specified id")
        || message.contains("Frame does not yet have a main execution context")
        || message.contains("Frame does not yet have the execution context")
}

async fn page_uses_browser_internal_url(page: &Page) -> Result<bool> {
    let url = page.url().await?.unwrap_or_default();
    Ok(matches_internal_browser_url(url.as_str()))
}

fn matches_internal_browser_url(url: &str) -> bool {
    [
        "about:",
        "chrome://",
        "chrome-search://",
        "chrome-extension://",
        "devtools://",
        "edge://",
    ]
    .iter()
    .any(|prefix| url.starts_with(prefix))
}

const PAGE_SNAPSHOT_JS: &str = r#"() => {
    const normalize = (value) => String(value ?? "").replace(/\s+/g, " ").trim();
    const truncate = (value, max) => {
        const text = normalize(value);
        if (text.length <= max) {
            return text;
        }
        return text.slice(0, Math.max(0, max - 1)) + "\u2026";
    };
    const visible = (el) => {
        const style = window.getComputedStyle(el);
        if (!style || style.visibility === "hidden" || style.display === "none") {
            return false;
        }
        const rect = el.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0;
    };
    const cssSegment = (el) => {
        if (el.id) {
            return `#${CSS.escape(el.id)}`;
        }
        const tag = el.tagName.toLowerCase();
        let index = 1;
        let sibling = el.previousElementSibling;
        while (sibling) {
            if (sibling.tagName === el.tagName) {
                index += 1;
            }
            sibling = sibling.previousElementSibling;
        }
        return `${tag}:nth-of-type(${index})`;
    };
    const cssPath = (el) => {
        const parts = [];
        let current = el;
        while (current && current.nodeType === Node.ELEMENT_NODE && parts.length < 6) {
            parts.unshift(cssSegment(current));
            if (current.id) {
                break;
            }
            current = current.parentElement;
        }
        return parts.join(" > ");
    };
    const inferRole = (el) => {
        const explicit = el.getAttribute("role");
        if (explicit) {
            return explicit;
        }
        const tag = el.tagName.toLowerCase();
        if (tag === "a" && el.href) {
            return "link";
        }
        if (tag === "button" || tag === "summary") {
            return "button";
        }
        if (tag === "select") {
            return "combobox";
        }
        if (tag === "textarea") {
            return "textbox";
        }
        if (tag === "input") {
            const type = (el.getAttribute("type") || "text").toLowerCase();
            if (type === "checkbox") {
                return "checkbox";
            }
            if (type === "radio") {
                return "radio";
            }
            if (type === "file") {
                return "file";
            }
            if (type === "button" || type === "submit" || type === "reset") {
                return "button";
            }
            return "textbox";
        }
        if (el.isContentEditable) {
            return "textbox";
        }
        return tag;
    };
    const labelFor = (el) => {
        const direct =
            el.getAttribute("aria-label") ||
            el.getAttribute("placeholder") ||
            el.getAttribute("title") ||
            "";
        if (direct) {
            return direct;
        }
        if (el.id) {
            const byFor = document.querySelector(`label[for="${CSS.escape(el.id)}"]`);
            if (byFor) {
                return byFor.innerText || byFor.textContent || "";
            }
        }
        const wrapped = el.closest("label");
        if (wrapped) {
            return wrapped.innerText || wrapped.textContent || "";
        }
        return el.innerText || el.textContent || "";
    };
    const interactiveSelector = [
        "a[href]",
        "button",
        "input",
        "select",
        "textarea",
        "summary",
        "[role]",
        "[contenteditable=\"\"]",
        "[contenteditable=\"true\"]"
    ].join(",");
    const uniqueInteractive = Array.from(document.querySelectorAll(interactiveSelector))
        .filter((el, index, items) => items.indexOf(el) === index)
        .filter(visible)
        .slice(0, 120);
    const elements = uniqueInteractive.map((el, index) => {
        const rect = el.getBoundingClientRect();
        const tag = el.tagName.toLowerCase();
        const value = typeof el.value === "string" ? el.value : "";
        return {
            id: `e${index + 1}`,
            selector: cssPath(el),
            tag,
            role: inferRole(el),
            label: truncate(labelFor(el), 160),
            text: truncate(el.innerText || el.textContent || value, 160),
            type: el.getAttribute("type") || "",
            href: el.href || "",
            placeholder: el.getAttribute("placeholder") || "",
            disabled: !!el.disabled,
            checked: !!el.checked,
            active: document.activeElement === el,
            value: truncate(value, 80),
            bbox: {
                x: Math.round(rect.x),
                y: Math.round(rect.y),
                width: Math.round(rect.width),
                height: Math.round(rect.height)
            }
        };
    });
    const headings = Array.from(document.querySelectorAll("h1,h2,h3"))
        .filter(visible)
        .slice(0, 24)
        .map((el) => ({
            tag: el.tagName.toLowerCase(),
            text: truncate(el.innerText || el.textContent || "", 200),
            selector: cssPath(el)
        }));
    const textPreview = truncate(
        document.body ? document.body.innerText || document.body.textContent || "" : "",
        2000
    );
    return {
        url: window.location.href,
        title: document.title || "",
        viewport: {
            width: Math.round(window.innerWidth || 0),
            height: Math.round(window.innerHeight || 0)
        },
        activeElement:
            document.activeElement && document.activeElement !== document.body
                ? cssPath(document.activeElement)
                : null,
        headings,
        textPreview,
        counts: {
            elements: elements.length,
            links: elements.filter((el) => el.role === "link").length,
            buttons: elements.filter((el) => el.role === "button").length,
            inputs: elements.filter((el) => ["textbox", "checkbox", "radio", "file", "combobox"].includes(el.role)).length
        },
        elements
    };
}"#;
