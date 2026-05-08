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
    const MAX_DEPTH = 30;
    const MAX_CHILDREN = 300;
    const MAX_NODES = 450;
    const MAX_TEXT = 120;
    const MAX_VALUE = 80;

    let nextRef = 1;
    let nodeCount = 0;
    let activeRef = null;

    const BADGE_TEXTS = new Set(["新", "热", "优", "沸", "荐"]);
    const CONTROL_TAGS = new Set(["input", "select", "textarea", "button"]);
    const SKIP_TAGS = new Set([
      "script",
      "style",
      "noscript",
      "template",
      "meta",
      "link",
      "source",
      "track"
    ]);

    const activeElement =
      document.activeElement && document.activeElement.nodeType === Node.ELEMENT_NODE
        ? document.activeElement
        : null;

    const normalize = (value) => String(value ?? "").replace(/\s+/g, " ").trim();

    const stripDecorativeGlyphs = (value) =>
      String(value ?? "")
        .replace(/[\uE000-\uF8FF]/g, " ")
        .replace(/[\u200B-\u200D\uFEFF]/g, " ");

    const cleanText = (value) => normalize(stripDecorativeGlyphs(value));

    const truncate = (value, max) => {
      const text = cleanText(value);
      if (!text) return "";
      if (text.length <= max) return text;
      return text.slice(0, Math.max(0, max - 3)) + "...";
    };

    const compact = (obj) =>
      Object.fromEntries(
        Object.entries(obj).filter(([, value]) => {
          if (value == null) return false;
          if (value === "") return false;
          if (value === false) return false;
          if (Array.isArray(value)) return value.length > 0;
          if (typeof value === "object") return Object.keys(value).length > 0;
          return true;
        })
      );

    const tagOf = (el) => (el.tagName || "").toLowerCase();

    const styleOf = (el) => {
      try {
        return el.ownerDocument.defaultView.getComputedStyle(el);
      } catch {
        return null;
      }
    };

    const directText = (el) =>
      cleanText(
        Array.from(el.childNodes || [])
          .filter((node) => node.nodeType === Node.TEXT_NODE)
          .map((node) => node.textContent || "")
          .join(" ")
      );

    const fullText = (el) => cleanText(el.innerText || el.textContent || "");

    const textFromIds = (rawIds, doc) =>
      cleanText(
        String(rawIds || "")
          .split(/\s+/)
          .map((id) => doc.getElementById(id))
          .filter(Boolean)
          .map((node) => node.innerText || node.textContent || "")
          .join(" ")
      );

    const isHiddenInput = (el) =>
      tagOf(el) === "input" && (el.getAttribute("type") || "").toLowerCase() === "hidden";

    const isHidden = (el) => {
      if (isHiddenInput(el)) return true;
      const style = styleOf(el);
      if (!style) return true;
      if (style.display === "none") return true;
      if (style.visibility === "hidden") return true;
      if (el.hasAttribute("hidden")) return true;
      return false;
    };

    const hasClickBehavior = (el) => {
      const style = styleOf(el);
      const tabindex = el.getAttribute("tabindex");
      return !!(
        (style && style.cursor === "pointer") ||
        typeof el.onclick === "function" ||
        el.hasAttribute("onclick") ||
        (tabindex != null && tabindex !== "-1") ||
        el.hasAttribute("aria-haspopup") ||
        el.hasAttribute("data-click") ||
        el.hasAttribute("data-action")
      );
    };

    const isNativeInteractive = (el) => {
      const tag = tagOf(el);
      if (["a", "button", "input", "select", "textarea", "summary"].includes(tag)) return true;
      if (el.hasAttribute("role")) return true;
      if (el.isContentEditable) return true;
      return false;
    };

    const isInteractive = (el) => isNativeInteractive(el) || hasClickBehavior(el);

    const isVisibleEnough = (el) => {
      if (isHidden(el)) return false;
      if (CONTROL_TAGS.has(tagOf(el)) && !isHiddenInput(el)) return true;
      if (isInteractive(el)) return true;

      const rect = el.getBoundingClientRect();
      if (rect.width > 0 || rect.height > 0) return true;
      if (fullText(el)) return true;
      return el.childElementCount > 0;
    };

    const roleOf = (el) => {
      const explicit = el.getAttribute("role");
      if (explicit) return explicit;

      const tag = tagOf(el);

      if (tag === "a") return "link";
      if (tag === "button" || tag === "summary") return "button";
      if (tag === "select") return "combobox";
      if (tag === "textarea") return "textbox";
      if (tag === "option") return "option";
      if (tag === "img") return "img";
      if (tag === "ul" || tag === "ol") return "list";
      if (tag === "li") return "listitem";
      if (tag === "nav") return "navigation";
      if (tag === "main") return "main";
      if (tag === "header") return "banner";
      if (tag === "footer") return "contentinfo";
      if (tag === "article") return "article";
      if (tag === "section") return "region";
      if (tag === "form") return "form";
      if (tag === "iframe") return "iframe";
      if (/^h[1-6]$/.test(tag)) return "heading";

      if (tag === "input") {
        const type = (el.getAttribute("type") || "text").toLowerCase();
        if (type === "checkbox") return "checkbox";
        if (type === "radio") return "radio";
        if (type === "file") return "file";
        if (type === "range") return "slider";
        if (["submit", "button", "reset"].includes(type)) return "button";
        return "textbox";
      }

      if (el.isContentEditable) return "textbox";
      return "generic";
    };

    const labelOf = (el, role) => {
      const doc = el.ownerDocument;
      const tag = tagOf(el);

      const ariaLabel = el.getAttribute("aria-label");
      if (ariaLabel) return ariaLabel;

      const labelledBy = el.getAttribute("aria-labelledby");
      if (labelledBy) {
        const text = textFromIds(labelledBy, doc);
        if (text) return text;
      }

      if (typeof el.labels !== "undefined" && el.labels && el.labels.length > 0) {
        const text = cleanText(
          Array.from(el.labels)
            .map((label) => label.innerText || label.textContent || "")
            .join(" ")
        );
        if (text) return text;
      }

      if (el.id) {
        const byFor = doc.querySelector(`label[for="${CSS.escape(el.id)}"]`);
        if (byFor) {
          const text = cleanText(byFor.innerText || byFor.textContent || "");
          if (text) return text;
        }
      }

      const wrapped = el.closest("label");
      if (wrapped) {
        const text = cleanText(wrapped.innerText || wrapped.textContent || "");
        if (text) return text;
      }

      const placeholder = el.getAttribute("placeholder");
      if (placeholder) return placeholder;

      const title = el.getAttribute("title");
      if (title) return title;

      const alt = el.getAttribute("alt");
      if (alt) return alt;

      if (tag === "input") {
        const type = (el.getAttribute("type") || "text").toLowerCase();
        if (["submit", "button", "reset"].includes(type) && el.value) {
          return el.value;
        }
        if (role === "textbox") {
          if (el.getAttribute("name")) return el.getAttribute("name");
          if (el.id) return `#${el.id}`;
        }
      }

      if (role === "textbox" || role === "combobox") {
        if (el.getAttribute("name")) return el.getAttribute("name");
        if (el.id) return `#${el.id}`;
      }

      if (role === "link" || role === "button" || role === "heading") {
        return fullText(el);
      }

      if (role === "generic" && hasClickBehavior(el)) {
        return fullText(el);
      }

      return "";
    };

    const stateOf = (el) =>
      compact({
        focused: activeElement === el,
        disabled: "disabled" in el ? !!el.disabled : false,
        checked: "checked" in el ? !!el.checked : false,
        selected:
          el.getAttribute("aria-selected") === "true"
            ? true
            : el.getAttribute("aria-selected") === "false"
              ? false
              : "selected" in el
                ? !!el.selected
                : false,
        expanded:
          el.getAttribute("aria-expanded") === "true"
            ? true
            : el.getAttribute("aria-expanded") === "false"
              ? false
              : null
      });

    const cssSegment = (el) => {
      const tag = tagOf(el) || "*";
      if (el.id) return `#${CSS.escape(el.id)}`;

      const nameAttr = el.getAttribute("name");
      let base = tag;

      if (nameAttr && /^[a-zA-Z0-9_-]+$/.test(nameAttr)) {
        base = `${tag}[name="${CSS.escape(nameAttr)}"]`;
      } else {
        const classes = Array.from(el.classList || [])
          .filter((cls) => /^[a-zA-Z0-9_-]+$/.test(cls))
          .slice(0, 1);
        if (classes.length > 0) {
          base = `${tag}.${CSS.escape(classes[0])}`;
        }
      }

      let index = 1;
      let sibling = el.previousElementSibling;
      while (sibling) {
        if (sibling.tagName === el.tagName) index += 1;
        sibling = sibling.previousElementSibling;
      }

      return `${base}:nth-of-type(${index})`;
    };

    const cssPath = (el) => {
      const parts = [];
      let current = el;
      let depth = 0;

      while (current && current.nodeType === Node.ELEMENT_NODE && depth < 5) {
        parts.unshift(cssSegment(current));
        if (current.id) break;
        current = current.parentElement;
        depth += 1;
      }

      return parts.join(" > ");
    };

    const bboxOf = (el) => {
      const rect = el.getBoundingClientRect();
      if (!(rect.width > 0 || rect.height > 0)) return null;
      return {
        x: Math.round(rect.x),
        y: Math.round(rect.y),
        width: Math.round(rect.width),
        height: Math.round(rect.height)
      };
    };

    const STRUCTURAL_ROLES = new Set([
      "navigation",
      "main",
      "banner",
      "contentinfo",
      "article",
      "region",
      "list",
      "listitem",
      "form",
      "iframe"
    ]);

    const LEAF_ROLES = new Set([
      "link",
      "button",
      "textbox",
      "checkbox",
      "radio",
      "combobox",
      "file",
      "slider",
      "heading",
      "img",
      "option"
    ]);

    const LOCATOR_ROLES = new Set([
      "link",
      "button",
      "textbox",
      "checkbox",
      "radio",
      "combobox",
      "file",
      "slider"
    ]);

    const BBOX_ROLES = new Set([
      "link",
      "button",
      "textbox",
      "checkbox",
      "radio",
      "combobox",
      "file",
      "slider",
      "img"
    ]);

    const collectChildElements = (el) => {
      const out = Array.from(el.children || []);

      if (el.shadowRoot) {
        out.push(...Array.from(el.shadowRoot.children || []));
      }

      if (tagOf(el) === "iframe") {
        try {
          const frameDoc = el.contentDocument;
          if (frameDoc && frameDoc.body) {
            out.push(...Array.from(frameDoc.body.children || []));
          }
        } catch {}
      }

      return out.slice(0, MAX_CHILDREN);
    };

    const isDecorativeGeneric = (node) =>
      node &&
      node.role === "generic" &&
      !node.name &&
      !node.url &&
      !node.value &&
      !node.children &&
      !cleanText(node.text || "");

    const isBadgeNode = (node) =>
      node &&
      node.role === "generic" &&
      !node.name &&
      !node.url &&
      !node.value &&
      !node.children &&
      BADGE_TEXTS.has(cleanText(node.text || ""));

    const mergeChild = (children, built) => {
      if (!built) return;
      if (built._children) {
        children.push(...built._children);
      } else {
        children.push(built);
      }
    };

    const buildNode = (el, depth, isRoot = false) => {
      if (!el || el.nodeType !== Node.ELEMENT_NODE) return null;
      if (depth > MAX_DEPTH || nodeCount >= MAX_NODES) return null;

      const tag = tagOf(el);
      if (!tag || SKIP_TAGS.has(tag) || isHiddenInput(el)) return null;
      if (!isRoot && isHidden(el)) return null;

      const role = roleOf(el);
      const inputType = (el.getAttribute("type") || "").toLowerCase();
      const mustKeep = (CONTROL_TAGS.has(tag) && !isHiddenInput(el)) || role === "button" || role === "link";
      const visibleEnough = isVisibleEnough(el);
      const genericInteractive = role === "generic" && isInteractive(el);

      const name = truncate(labelOf(el, role), MAX_TEXT);

      let text = truncate(
        role === "link" || role === "button" || role === "heading"
          ? fullText(el)
          : directText(el),
        MAX_TEXT
      );

      if (name && text && cleanText(name) === cleanText(text)) {
        text = "";
      }

      let value = "";
      if (tag === "textarea" || tag === "select") {
        value = truncate(el.value || "", MAX_VALUE);
      } else if (
        tag === "input" &&
        !["hidden", "checkbox", "radio", "file", "submit", "button", "reset"].includes(inputType)
      ) {
        value = inputType === "password" ? (el.value ? "[masked]" : "") : truncate(el.value || "", MAX_VALUE);
      } else if (el.isContentEditable) {
        value = truncate(fullText(el), MAX_VALUE);
      }

      const url =
        (tag === "a" && (el.href || el.getAttribute("href") || "")) ||
        (tag === "img" && (el.src || el.getAttribute("src") || "")) ||
        (tag === "iframe" && (el.src || el.getAttribute("src") || "")) ||
        "";

      const level = /^h[1-6]$/.test(tag)
        ? Number(tag.slice(1))
        : Number(el.getAttribute("aria-level") || 0) || null;

      const state = stateOf(el);
      const locator =
        LOCATOR_ROLES.has(role) || genericInteractive ? { css: cssPath(el) } : null;
      const bbox =
        BBOX_ROLES.has(role) || (genericInteractive && visibleEnough) ? bboxOf(el) : null;

      const children = [];
      const badges = [];

      if (nodeCount < MAX_NODES && !LEAF_ROLES.has(role)) {
        for (const child of collectChildElements(el)) {
          if (nodeCount >= MAX_NODES) break;
          const built = buildNode(child, depth + 1, false);
          if (!built) continue;

          const builtChildren = built._children ? built._children : [built];
          for (const item of builtChildren) {
            if (role === "listitem" && isBadgeNode(item)) {
              badges.push(cleanText(item.text));
              continue;
            }
            if (isDecorativeGeneric(item)) {
              continue;
            }
            children.push(item);
          }
        }
      }

      const keepSelf =
        isRoot ||
        mustKeep ||
        visibleEnough ||
        STRUCTURAL_ROLES.has(role) ||
        LEAF_ROLES.has(role) ||
        genericInteractive ||
        !!name ||
        !!text ||
        !!value ||
        !!url ||
        badges.length > 0 ||
        children.length > 0;

      if (!keepSelf) return null;

      const ref = `e${nextRef++}`;
      nodeCount += 1;
      if (activeElement === el) activeRef = ref;

      const keepTag =
        role === "generic" ||
        CONTROL_TAGS.has(tag) ||
        tag === "img" ||
        tag === "iframe";

      const node = compact({
        ref,
        role,
        tag: keepTag ? tag : "",
        name,
        text,
        value,
        url,
        inputType,
        placeholder: truncate(el.getAttribute("placeholder") || "", 60),
        level,
        badges: Array.from(new Set(badges)),
        interactive: genericInteractive,
        locator,
        state,
        bbox,
        children
      });

      const flattenable =
        !isRoot &&
        role === "generic" &&
        !genericInteractive &&
        !mustKeep &&
        !node.name &&
        !node.text &&
        !node.value &&
        !node.url &&
        !node.inputType &&
        !node.placeholder &&
        !node.level &&
        !node.badges &&
        !node.locator &&
        !node.state &&
        !node.bbox;

      if (flattenable) {
        if (children.length === 1) return children[0];
        return { _children: children };
      }

      return node;
    };

    const rootElement = document.body || document.documentElement;
    const root = rootElement ? buildNode(rootElement, 0, true) : null;

    return compact({
      url: window.location.href,
      title: document.title || "",
      viewport: {
        width: Math.round(window.innerWidth || 0),
        height: Math.round(window.innerHeight || 0)
      },
      activeRef,
      root
    });
  }"#;
