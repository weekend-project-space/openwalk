use super::{
    actor::BrowserActor,
    types::{BrowserValue, Locator},
    util::locator_name,
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
