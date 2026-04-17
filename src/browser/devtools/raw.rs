use super::super::{
    actor::BrowserActor,
    types::{BrowserValue, RawCdpResult},
    util::serialize_to_browser_value,
    *,
};

#[derive(Debug, Clone)]
struct RawCommand {
    method: String,
    params: serde_json::Value,
}

impl BrowserActor {
    pub(in crate::browser) async fn cdp(
        &mut self,
        method: String,
        params: String,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        let params_value: serde_json::Value =
            serde_json::from_str(params.as_str()).context("invalid CDP params JSON")?;
        let response = page
            .execute(RawCommand {
                method: method.clone(),
                params: params_value,
            })
            .await
            .with_context(|| format!("failed to execute CDP method `{method}`"))?;
        let output = RawCdpResult {
            method,
            result: response.result,
        };
        serialize_to_browser_value(&output, "failed to serialize CDP result")
    }
}

impl Method for RawCommand {
    fn identifier(&self) -> MethodId {
        self.method.clone().into()
    }
}

impl Command for RawCommand {
    type Response = serde_json::Value;
}

impl Serialize for RawCommand {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.params.serialize(serializer)
    }
}
