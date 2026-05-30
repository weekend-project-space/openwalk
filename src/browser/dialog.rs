use anyhow::anyhow;
use chromiumoxide::cdp::browser_protocol::page::HandleJavaScriptDialogParams;

use super::{actor::BrowserActor, types::BrowserValue, *};

impl BrowserActor {
    pub(super) async fn dialog_accept(
        &mut self,
        prompt_text: Option<String>,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let mut params = HandleJavaScriptDialogParams::builder().accept(true);
        if let Some(prompt_text) = prompt_text {
            params = params.prompt_text(prompt_text);
        }

        page.execute(
            params
                .build()
                .map_err(|err| anyhow!("failed to build dialog accept request: {err}"))?,
        )
        .await
        .context("failed to accept JavaScript dialog")?;

        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn dialog_dismiss(&mut self) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.execute(
            HandleJavaScriptDialogParams::builder()
                .accept(false)
                .build()
                .map_err(|err| anyhow!("failed to build dialog dismiss request: {err}"))?,
        )
        .await
        .context("failed to dismiss JavaScript dialog")?;

        Ok(BrowserValue::Boolean(true))
    }
}
