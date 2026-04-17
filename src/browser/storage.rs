use super::{
    actor::BrowserActor,
    types::{BrowserValue, StorageEntry},
    util::serialize_to_browser_value,
    *,
};

impl BrowserActor {
    pub(super) async fn storage_get(&mut self, storage: &str, key: String) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let expression = format!("() => window[{storage:?}].getItem({key:?}) ?? ''");
        let value: String = page
            .evaluate(expression.as_str())
            .await
            .with_context(|| format!("failed to read {storage} key `{key}`"))?
            .into_value()
            .unwrap_or_default();
        Ok(BrowserValue::String(value))
    }

    pub(super) async fn storage_set(
        &mut self,
        storage: &str,
        key: String,
        value: String,
    ) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let expression = format!(
            "() => {{ window[{storage:?}].setItem({key:?}, {value:?}); return window[{storage:?}].getItem({key:?}) ?? ''; }}"
        );
        let stored: String = page
            .evaluate(expression.as_str())
            .await
            .with_context(|| format!("failed to set {storage} key `{key}`"))?
            .into_value()
            .unwrap_or_default();
        Ok(BrowserValue::String(stored))
    }

    pub(super) async fn storage_remove(
        &mut self,
        storage: &str,
        key: String,
    ) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let expression =
            format!("() => {{ window[{storage:?}].removeItem({key:?}); return true; }}");
        page.evaluate(expression.as_str())
            .await
            .with_context(|| format!("failed to remove {storage} key `{key}`"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn storage_clear(&mut self, storage: &str) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let expression = format!("() => {{ window[{storage:?}].clear(); return true; }}");
        page.evaluate(expression.as_str())
            .await
            .with_context(|| format!("failed to clear {storage}"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn storage_items(&mut self, storage: &str) -> Result<BrowserValue> {
        let page = self.require_page_ready().await?;
        let expression = format!(
            "(() => Object.entries(window[{storage:?}]).map(([key, value]) => ({{ key, value }})))()"
        );
        let items: Vec<StorageEntry> = page
            .evaluate(expression.as_str())
            .await
            .with_context(|| format!("failed to list {storage} items"))?
            .into_value()
            .unwrap_or_default();
        serialize_to_browser_value(&items, "failed to serialize storage items")
    }

    pub(super) async fn cookies(&mut self) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let cookies = page.get_cookies().await.context("failed to read cookies")?;
        serialize_to_browser_value(&cookies, "failed to serialize cookies")
    }

    pub(super) async fn cookie_get(&mut self, name: String) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let cookies = page.get_cookies().await.context("failed to read cookies")?;
        let cookie = cookies.into_iter().find(|cookie| cookie.name == name);
        serialize_to_browser_value(&cookie, "failed to serialize cookie")
    }

    pub(super) async fn cookie_set(
        &mut self,
        name: String,
        value: String,
        url: Option<String>,
        domain: Option<String>,
        path: Option<String>,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let mut cookie = CookieParam::new(name.clone(), value.clone());
        cookie.url = url;
        cookie.domain = domain;
        cookie.path = path;
        page.set_cookie(cookie)
            .await
            .with_context(|| format!("failed to set cookie `{name}`"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn cookie_delete(
        &mut self,
        name: String,
        url: Option<String>,
        domain: Option<String>,
        path: Option<String>,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let mut builder = DeleteCookiesParams::builder().name(name.clone());
        if let Some(url) = url {
            builder = builder.url(url);
        }
        if let Some(domain) = domain {
            builder = builder.domain(domain);
        }
        if let Some(path) = path {
            builder = builder.path(path);
        }
        page.delete_cookie(builder.build().map_err(anyhow::Error::msg)?)
            .await
            .with_context(|| format!("failed to delete cookie `{name}`"))?;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn cookies_clear(&mut self) -> Result<BrowserValue> {
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| anyhow!("browser is not running"))?;
        browser
            .clear_cookies()
            .await
            .context("failed to clear browser cookies")?;
        Ok(BrowserValue::Boolean(true))
    }
}
