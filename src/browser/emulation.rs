use super::{actor::BrowserActor, types::BrowserValue, *};

impl BrowserActor {
    pub(super) async fn set_viewport(&mut self, width: i64, height: i64) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        page.execute(SetDeviceMetricsOverrideParams::new(
            width, height, 1.0, false,
        ))
        .await
        .with_context(|| format!("failed to set viewport to {width}x{height}"))?;
        Ok(BrowserValue::String(format!("{width}x{height}")))
    }
}
