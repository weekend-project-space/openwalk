use std::collections::HashMap;

use super::{
    actor::BrowserActor,
    types::{BrowserLaunchMode, BrowserTabInfo, BrowserValue},
    util::serialize_to_browser_value,
    *,
};

impl BrowserActor {
    const TAB_ID_ABBREV_MIN: usize = 7;

    fn remembered_active_target_id(&self) -> Option<String> {
        let current = self
            .active_page
            .and_then(|index| self.pages.get(index))
            .map(|page| page.target_id().as_ref().to_string());
        if current.is_some() {
            return current;
        }

        match &self.mode {
            BrowserLaunchMode::Session(session) => session
                .active_target_id()
                .map(std::borrow::ToOwned::to_owned),
            BrowserLaunchMode::Ephemeral(_) => None,
        }
    }

    pub(super) fn persist_active_target_id(
        &mut self,
        active_target_id: Option<String>,
    ) -> Result<()> {
        if let BrowserLaunchMode::Session(session) = &mut self.mode {
            session.set_active_target_id(active_target_id)?;
        }
        Ok(())
    }

    pub(super) fn persist_current_active_page(&mut self) -> Result<()> {
        let active_target_id = self
            .active_page
            .and_then(|index| self.pages.get(index))
            .map(|page| page.target_id().as_ref().to_string());
        self.persist_active_target_id(active_target_id)
    }

    async fn tab_snapshot(
        &self,
        page: &Page,
        short_id: String,
        active: bool,
    ) -> Result<BrowserTabInfo> {
        Ok(BrowserTabInfo {
            id: short_id,
            url: page.url().await.unwrap_or(None).unwrap_or_default(),
            title: page.get_title().await.unwrap_or(None).unwrap_or_default(),
            active,
        })
    }

    pub(super) async fn refresh_pages_from_connected_browser(&mut self) -> Result<()> {
        let browser = self.browser.as_ref().expect("browser should be available");
        let previous_active = self.remembered_active_target_id();
        let latest = browser
            .pages()
            .await
            .context("failed to query browser tabs")?;
        debug_log_pages_snapshot(
            "refresh_pages_from_connected_browser",
            latest.as_slice(),
            previous_active.as_deref(),
        )
        .await;
        self.pages = latest;
        self.active_page = if self.pages.is_empty() {
            None
        } else if let Some(id) = previous_active {
            self.pages
                .iter()
                .position(|page| page.target_id().as_ref() == id)
                .or(Some(0))
        } else {
            Some(0)
        };
        self.persist_current_active_page()?;
        Ok(())
    }

    pub(super) async fn sync_pages_from_browser(&mut self) -> Result<()> {
        if self.browser.is_none() {
            if matches!(self.mode, BrowserLaunchMode::Session(_)) {
                self.ensure_browser_launched().await?;
            } else {
                bail!("no active browser page. Call `browser-open` first");
            }
        }

        self.refresh_pages_from_connected_browser().await?;
        self.require_page()
            .map(|_| ())
            .map_err(|_| anyhow!("no active browser page. Call `browser-open` first"))
    }

    pub(super) async fn tabs(&mut self) -> Result<BrowserValue> {
        self.sync_pages_from_browser().await?;
        let short_ids = tab_short_ids(self.pages.as_slice(), Self::TAB_ID_ABBREV_MIN);
        let mut tabs = Vec::with_capacity(self.pages.len());
        for (index, page) in self.pages.iter().enumerate() {
            let id = page.target_id().as_ref().to_string();
            let short_id = short_ids.get(&id).cloned().unwrap_or(id);
            tabs.push(
                self.tab_snapshot(page, short_id, self.active_page == Some(index))
                    .await?,
            );
        }
        serialize_to_browser_value(&tabs, "failed to serialize tabs")
    }

    pub(super) async fn new_tab(&mut self, url: Option<String>) -> Result<BrowserValue> {
        self.sync_pages_from_browser()
            .await
            .context("`tab-new` requires an opened browser page")?;
        let browser = self.browser.as_ref().expect("browser should be available");
        let page = browser
            .new_page("about:blank")
            .await
            .context("failed to create browser tab")?;
        self.ensure_network_tracking_for_page(page.clone()).await?;
        self.ensure_console_tracking_for_page(page.clone()).await?;
        if let Some(url) = url {
            page.goto(url.as_str())
                .await
                .with_context(|| format!("failed to navigate tab to `{url}`"))?;
        }
        // Re-sync from browser so returned index always reflects current browser tab order.
        self.pages.push(page.clone());
        self.active_page = Some(self.pages.len() - 1);
        page.bring_to_front().await.ok();
        self.refresh_pages_from_connected_browser().await?;

        let index = self.active_page.unwrap_or(0);
        let page = self
            .pages
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow!("active tab index `{index}` is out of range"))?;
        let short_ids = tab_short_ids(self.pages.as_slice(), Self::TAB_ID_ABBREV_MIN);
        let id = page.target_id().as_ref().to_string();
        let short_id = short_ids.get(&id).cloned().unwrap_or(id);
        let info = self.tab_snapshot(&page, short_id, true).await?;
        serialize_to_browser_value(&info, "failed to serialize new tab info")
    }

    pub(super) async fn switch_tab(&mut self, tab: String) -> Result<BrowserValue> {
        self.sync_pages_from_browser().await?;
        let index = self.resolve_tab_reference_index(tab.as_str())?;
        let page = self
            .pages
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow!("tab index `{index}` is out of range"))?;
        self.ensure_network_tracking_for_page(page.clone()).await?;
        self.ensure_console_tracking_for_page(page.clone()).await?;
        page.bring_to_front()
            .await
            .with_context(|| format!("failed to activate tab `{index}`"))?;
        self.active_page = Some(index);
        self.persist_current_active_page()?;
        let short_ids = tab_short_ids(self.pages.as_slice(), Self::TAB_ID_ABBREV_MIN);
        let id = page.target_id().as_ref().to_string();
        let short_id = short_ids.get(&id).cloned().unwrap_or(id);
        let info = self.tab_snapshot(&page, short_id, true).await?;
        serialize_to_browser_value(&info, "failed to serialize selected tab info")
    }

    pub(super) async fn close_tab(&mut self, tab: Option<String>) -> Result<BrowserValue> {
        self.sync_pages_from_browser().await?;
        let index = if let Some(tab) = tab {
            self.resolve_tab_reference_index(tab.as_str())?
        } else {
            self.active_page.ok_or_else(|| anyhow!("no tab to close"))?
        };
        if index >= self.pages.len() {
            bail!("tab index `{index}` is out of range");
        }
        let page = self.pages.remove(index);
        let page_id = page.target_id().as_ref().to_string();
        page.close()
            .await
            .with_context(|| format!("failed to close tab `{index}`"))?;
        self.observed_network_targets.remove(page_id.as_str());
        self.clear_console_page_state(page_id.as_str());
        if self
            .trace_session
            .as_ref()
            .map(|session| session.page_id.as_str() == page_id.as_str())
            .unwrap_or(false)
        {
            self.trace_session = None;
        }
        if self.pages.is_empty() {
            self.active_page = None;
        } else {
            self.active_page = Some(index.min(self.pages.len() - 1));
        }
        self.persist_current_active_page()?;
        self.refresh_pages_from_connected_browser().await?;
        Ok(BrowserValue::Boolean(true))
    }

    fn resolve_tab_reference_index(&self, raw_tab_ref: &str) -> Result<usize> {
        let ids = self
            .pages
            .iter()
            .map(|page| page.target_id().as_ref().to_string())
            .collect::<Vec<_>>();
        resolve_tab_reference_index_in_ids(raw_tab_ref, &ids, Self::TAB_ID_ABBREV_MIN)
    }
}

fn resolve_tab_reference_index_in_ids(
    raw_tab_ref: &str,
    ids: &[String],
    min_len: usize,
) -> Result<usize> {
    let tab_ref = raw_tab_ref.trim();
    if tab_ref.is_empty() {
        bail!("tab id cannot be empty");
    }

    if let Some(index) = ids
        .iter()
        .position(|id| id.as_str().eq_ignore_ascii_case(tab_ref))
    {
        return Ok(index);
    }

    let normalized = tab_ref.to_ascii_uppercase();
    let matches = ids
        .iter()
        .enumerate()
        .filter(|(_, id)| id.to_ascii_uppercase().starts_with(normalized.as_str()))
        .map(|(index, id)| (index, id.clone()))
        .collect::<Vec<_>>();

    if matches.is_empty() {
        if let Ok(index) = tab_ref.parse::<usize>() {
            if index < ids.len() {
                return Ok(index);
            }
            bail!(
                "tab index `{index}` is out of range (available: 0..{})",
                ids.len().saturating_sub(1)
            );
        }
        bail!("tab `{tab_ref}` was not found. Run `tab-list` to inspect available tab ids");
    }

    if matches.len() == 1 {
        return Ok(matches[0].0);
    }

    if let Ok(index) = tab_ref.parse::<usize>() {
        if index < ids.len() {
            return Ok(index);
        }
    }

    let short_ids = tab_short_ids_from_ids(ids, min_len);
    let candidates = matches
        .iter()
        .map(|(_, id)| short_ids.get(id).cloned().unwrap_or_else(|| id.clone()))
        .collect::<Vec<_>>()
        .join(", ");
    bail!("tab id `{tab_ref}` is ambiguous, matches: {candidates}");
}

fn tab_short_ids(pages: &[Page], min_len: usize) -> HashMap<String, String> {
    let ids = pages
        .iter()
        .map(|page| page.target_id().as_ref().to_string())
        .collect::<Vec<_>>();
    tab_short_ids_from_ids(&ids, min_len)
}

fn tab_short_ids_from_ids(ids: &[String], min_len: usize) -> HashMap<String, String> {
    let mut short_ids = HashMap::with_capacity(ids.len());

    for id in ids {
        let short = shortest_unique_prefix(id.as_str(), ids, min_len);
        short_ids.insert(id.clone(), short);
    }

    short_ids
}

fn shortest_unique_prefix(id: &str, all_ids: &[String], min_len: usize) -> String {
    let start = min_len.min(id.len());
    for len in start..=id.len() {
        let prefix = &id[..len];
        let collisions = all_ids
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .count();
        if collisions == 1 {
            return prefix.to_string();
        }
    }
    id.to_string()
}

async fn debug_log_pages_snapshot(
    context: &str,
    pages: &[Page],
    preferred_active_id: Option<&str>,
) {
    if !super::util::env_flag_is_truthy("OPENWALK_DEBUG_TAB_PAGES") {
        return;
    }

    eprintln!("[openwalk][tab-debug] {context}: pages={}", pages.len());
    for (index, page) in pages.iter().enumerate() {
        let id = page.target_id().as_ref().to_string();
        let url = page.url().await.unwrap_or(None).unwrap_or_default();
        let title = page.get_title().await.unwrap_or(None).unwrap_or_default();
        let preferred_active = preferred_active_id
            .map(|target| target == id.as_str())
            .unwrap_or(false);
        eprintln!(
            "[openwalk][tab-debug] page index={index} id={id} preferred_active={preferred_active} url={url:?} title={title:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn shortest_unique_prefix_respects_min_length() {
        let all = ids(&["ABCDEF1234", "ABC9999999", "F00BAA7777"]);
        assert_eq!(shortest_unique_prefix("ABCDEF1234", &all, 3), "ABCD");
        assert_eq!(shortest_unique_prefix("F00BAA7777", &all, 3), "F00");
    }

    #[test]
    fn resolve_tab_reference_supports_case_insensitive_full_id() {
        let all = ids(&["A46F3789ABCDEF00", "48F64A412345678"]);
        let index = resolve_tab_reference_index_in_ids("a46f3789abcdef00", &all, 7)
            .expect("full id should resolve");
        assert_eq!(index, 0);
    }

    #[test]
    fn resolve_tab_reference_supports_unique_short_id() {
        let all = ids(&["A46F3789ABCDEF00", "48F64A412345678"]);
        let index = resolve_tab_reference_index_in_ids("A46F378", &all, 7)
            .expect("short id should resolve");
        assert_eq!(index, 0);
    }

    #[test]
    fn resolve_tab_reference_prefers_numeric_short_id_over_index_parse() {
        let all = ids(&["5672990ABCDEF000", "6BDB289ABCDEF000"]);
        let index = resolve_tab_reference_index_in_ids("5672990", &all, 7)
            .expect("numeric short id should resolve as id");
        assert_eq!(index, 0);
    }

    #[test]
    fn resolve_tab_reference_falls_back_to_index_when_id_not_found() {
        let all = ids(&["A46F3789ABCDEF00", "48F64A412345678", "D7BF8BB12345678"]);
        let index =
            resolve_tab_reference_index_in_ids("1", &all, 7).expect("index fallback should work");
        assert_eq!(index, 1);
    }

    #[test]
    fn resolve_tab_reference_reports_ambiguous_id() {
        let all = ids(&["ABC1111111111111", "ABC2222222222222", "F00BAA777777777"]);
        let error = resolve_tab_reference_index_in_ids("ABC", &all, 7)
            .expect_err("ambiguous prefix should fail");
        let message = error.to_string();
        assert!(message.contains("ambiguous"));
    }

    #[test]
    fn resolve_tab_reference_reports_out_of_range_index() {
        let all = ids(&["A46F3789ABCDEF00", "48F64A412345678"]);
        let error = resolve_tab_reference_index_in_ids("9", &all, 7)
            .expect_err("out-of-range index should fail");
        let message = error.to_string();
        assert!(message.contains("out of range"));
    }

    #[test]
    fn resolve_tab_reference_rejects_empty_input() {
        let all = ids(&["A46F3789ABCDEF00", "48F64A412345678"]);
        let error = resolve_tab_reference_index_in_ids("   ", &all, 7)
            .expect_err("empty input should fail");
        let message = error.to_string();
        assert!(message.contains("cannot be empty"));
    }
}
