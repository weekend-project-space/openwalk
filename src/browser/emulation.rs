use super::{actor::BrowserActor, types::BrowserValue, *};

impl BrowserActor {
    pub(super) async fn resize_window(&mut self, width: i64, height: i64) -> Result<BrowserValue> {
        if width <= 0 || height <= 0 {
            bail!("browser-resize expects positive width and height");
        }

        let page = self.ensure_active_page().await?;
        let window = page
            .execute(GetWindowForTargetParams::builder().build())
            .await
            .context("failed to find browser window for active page")?;
        let bounds = Bounds::builder()
            .width(width)
            .height(height)
            .window_state(WindowState::Normal)
            .build();
        page.execute(SetWindowBoundsParams::new(window.result.window_id, bounds))
            .await
            .with_context(|| format!("failed to resize browser window to {width}x{height}"))?;

        Ok(BrowserValue::String(format!("{width}x{height}")))
    }
}
