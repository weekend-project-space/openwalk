use super::{actor::BrowserActor, types::BrowserValue, util::json_to_browser_value, *};

impl BrowserActor {
    pub(super) async fn wait_timeout(&mut self, ms: u64) -> Result<BrowserValue> {
        sleep(Duration::from_millis(ms)).await;
        Ok(BrowserValue::Boolean(true))
    }

    pub(super) async fn wait_function(&mut self, expression: String) -> Result<BrowserValue> {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let page = self.require_page_ready().await?;
            let value: serde_json::Value = page
                .evaluate(expression.as_str())
                .await
                .context("failed to evaluate wait function")?
                .into_value()
                .unwrap_or(serde_json::Value::Bool(false));
            if value.as_bool().unwrap_or(false) {
                return Ok(BrowserValue::Boolean(true));
            }
            sleep(Duration::from_millis(150)).await;
        }
        bail!("timed out waiting for browser function to become truthy")
    }

    pub(super) async fn eval(&mut self, expression: String) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let value: serde_json::Value = page
            .evaluate(expression.as_str())
            .await
            .context("failed to evaluate browser expression")?
            .into_value()
            .context("browser expression returned a non-serializable value")?;

        Ok(json_to_browser_value(value))
    }
}
