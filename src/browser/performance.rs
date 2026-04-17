use super::{
    actor::BrowserActor,
    types::{BrowserMetricsInfo, BrowserValue},
    util::serialize_to_browser_value,
    *,
};

impl BrowserActor {
    pub(super) async fn performance_metrics(&mut self) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let layout = page
            .execute(GetLayoutMetricsParams::default())
            .await
            .context("failed to query layout metrics")?;
        let perf = page
            .execute(GetMetricsParams::default())
            .await
            .context("failed to query performance metrics")?;
        let metrics = BrowserMetricsInfo {
            url: page.url().await?.unwrap_or_default(),
            css_layout_viewport: serde_json::to_value(layout.result.css_layout_viewport)
                .context("failed to encode layout viewport")?,
            css_visual_viewport: serde_json::to_value(layout.result.css_visual_viewport)
                .context("failed to encode visual viewport")?,
            css_content_size: serde_json::to_value(perf.result.metrics)
                .context("failed to encode performance metrics")?,
        };
        serialize_to_browser_value(&metrics, "failed to serialize performance metrics")
    }
}
