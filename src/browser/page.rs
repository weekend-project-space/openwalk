use super::{
    actor::BrowserActor,
    types::{BrowserValue, Locator},
    util::{json_to_browser_value, locator_name},
    *,
};

impl BrowserActor {
    pub(super) async fn goto(&mut self, url: String) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        if page_uses_browser_internal_url(&page).await? {
            bail!("cannot `page-goto` from an internal browser page; call `browser-open` instead");
        }

        page.goto(url.as_str())
            .await
            .with_context(|| format!("failed to navigate current page to `{url}`"))?;
        Ok(BrowserValue::String(page.url().await?.unwrap_or(url)))
    }

    pub(super) async fn back(&mut self) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        page.evaluate("history.back()")
            .await
            .context("failed to navigate back")?;
        page.wait_for_navigation()
            .await
            .context("failed while waiting for back navigation")?;
        Ok(BrowserValue::String(page.url().await?.unwrap_or_default()))
    }

    pub(super) async fn forward(&mut self) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        page.evaluate("history.forward()")
            .await
            .context("failed to navigate forward")?;
        page.wait_for_navigation()
            .await
            .context("failed while waiting for forward navigation")?;
        Ok(BrowserValue::String(page.url().await?.unwrap_or_default()))
    }

    pub(super) async fn reload(&mut self) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        page.reload().await.context("failed to reload the page")?;
        Ok(BrowserValue::String(page.url().await?.unwrap_or_default()))
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
