use anyhow::{bail, Context, Result};

use super::{actor::BrowserActor, types::BrowserValue};

impl BrowserActor {
    pub(super) async fn open(&mut self, url: String, new_tab: bool) -> Result<BrowserValue> {
        if !new_tab && (self.browser.is_some() || !self.pages.is_empty()) {
            bail!("browser is already open; call `browser-close` before `browser-open`");
        }

        self.ensure_browser_launched().await?;
        if self.has_single_placeholder_page().await? {
            if let Some(page) = self.pages.pop() {
                let page_id = page.target_id().as_ref().to_string();
                let _ = page.close().await;
                self.observed_network_targets.remove(page_id.as_str());
                self.clear_console_page_state(page_id.as_str());
            }
            self.active_page = None;
            self.persist_current_active_page().ok();
        }

        if !new_tab && !self.pages.is_empty() {
            bail!("browser is already open; call `browser-close` before `browser-open`");
        }

        let browser = self.browser.as_ref().expect("browser should be available");
        let page = browser
            .new_page("about:blank")
            .await
            .context("failed to create a fresh browser page")?;
        self.ensure_network_tracking_for_page(page.clone()).await?;
        self.ensure_console_tracking_for_page(page.clone()).await?;

        self.pages.push(page.clone());
        self.active_page = Some(self.pages.len() - 1);
        page.bring_to_front().await.ok();
        self.persist_current_active_page().ok();
        let final_url = super::page::navigate_page_to_url(&page, url.as_str()).await?;

        Ok(BrowserValue::String(
            page.get_title().await?.unwrap_or(final_url),
        ))
    }

    async fn has_single_placeholder_page(&self) -> Result<bool> {
        if self.pages.len() != 1 {
            return Ok(false);
        }

        let page = self
            .pages
            .first()
            .expect("single-page check should guarantee a page exists");
        let current_url = page.url().await.unwrap_or(None).unwrap_or_default();
        Ok(matches!(
            current_url.as_str(),
            "" | "about:blank" | "chrome://newtab/" | "chrome://new-tab-page/"
        ))
    }
}
