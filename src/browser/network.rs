use base64::{engine::general_purpose::STANDARD, Engine as _};
use chromiumoxide::cdp::browser_protocol::network::{
    EnableParams, EventLoadingFailed, EventLoadingFinished, EventRequestWillBeSent,
    EventResponseReceived, GetResponseBodyParams,
};

use super::{
    actor::BrowserActor,
    types::{BrowserValue, NetworkEntry, NetworkRequestInfo, NetworkResponseInfo, NetworkState},
    util::serialize_to_browser_value,
    *,
};

const NETWORK_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const NETWORK_POLL_INTERVAL: Duration = Duration::from_millis(150);
const NETWORK_TOTAL_BUFFER_SIZE: i64 = 50 * 1024 * 1024;
const NETWORK_RESOURCE_BUFFER_SIZE: i64 = 5 * 1024 * 1024;

impl BrowserActor {
    pub(super) async fn ensure_network_tracking_for_page(&mut self, page: Page) -> Result<()> {
        let page_id = page.target_id().as_ref().to_string();
        if self.observed_network_targets.contains(page_id.as_str()) {
            return Ok(());
        }

        page.execute(
            EnableParams::builder()
                .max_total_buffer_size(NETWORK_TOTAL_BUFFER_SIZE)
                .max_resource_buffer_size(NETWORK_RESOURCE_BUFFER_SIZE)
                .enable_durable_messages(true)
                .build(),
        )
        .await
        .with_context(|| format!("failed to enable network tracking for page `{page_id}`"))?;

        let mut request_events = page
            .event_listener::<EventRequestWillBeSent>()
            .await
            .with_context(|| format!("failed to subscribe to request events for `{page_id}`"))?;
        let mut response_events = page
            .event_listener::<EventResponseReceived>()
            .await
            .with_context(|| format!("failed to subscribe to response events for `{page_id}`"))?;
        let mut finished_events = page
            .event_listener::<EventLoadingFinished>()
            .await
            .with_context(|| format!("failed to subscribe to loading events for `{page_id}`"))?;
        let mut failed_events = page
            .event_listener::<EventLoadingFailed>()
            .await
            .with_context(|| format!("failed to subscribe to failure events for `{page_id}`"))?;

        let network_state = self.network_state.clone();
        let observed_page_id = page_id.clone();
        let task = tokio::spawn(async move {
            let mut requests_closed = false;
            let mut responses_closed = false;
            let mut finished_closed = false;
            let mut failed_closed = false;

            while !(requests_closed && responses_closed && finished_closed && failed_closed) {
                tokio::select! {
                    event = request_events.next(), if !requests_closed => {
                        match event {
                            Some(event) => {
                                if let Ok(mut state) = network_state.lock() {
                                    state.record_request(
                                        observed_page_id.as_str(),
                                        event.as_ref().clone(),
                                    );
                                }
                            }
                            None => requests_closed = true,
                        }
                    }
                    event = response_events.next(), if !responses_closed => {
                        match event {
                            Some(event) => {
                                if let Ok(mut state) = network_state.lock() {
                                    state.record_response(
                                        observed_page_id.as_str(),
                                        event.as_ref().clone(),
                                    );
                                }
                            }
                            None => responses_closed = true,
                        }
                    }
                    event = finished_events.next(), if !finished_closed => {
                        match event {
                            Some(event) => {
                                if let Ok(mut state) = network_state.lock() {
                                    state.mark_finished(
                                        observed_page_id.as_str(),
                                        event.as_ref().clone(),
                                    );
                                }
                            }
                            None => finished_closed = true,
                        }
                    }
                    event = failed_events.next(), if !failed_closed => {
                        match event {
                            Some(event) => {
                                if let Ok(mut state) = network_state.lock() {
                                    state.mark_failed(
                                        observed_page_id.as_str(),
                                        event.as_ref().clone(),
                                    );
                                }
                            }
                            None => failed_closed = true,
                        }
                    }
                }
            }
        });

        self.observed_network_targets.insert(page_id);
        self.network_listener_tasks.push(task);
        Ok(())
    }

    pub(super) async fn network_requests(&mut self) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        self.ensure_network_tracking_for_page(page.clone()).await?;
        let page_id = page.target_id().as_ref().to_string();
        let entries = self.network_page_entries(page_id.as_str())?;
        serialize_to_browser_value(&entries, "failed to serialize network requests")
    }

    pub(super) async fn network_wait_response(
        &mut self,
        url_contains: String,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        self.ensure_network_tracking_for_page(page.clone()).await?;
        let page_id = page.target_id().as_ref().to_string();
        let deadline = Instant::now() + NETWORK_WAIT_TIMEOUT;

        while Instant::now() < deadline {
            if let Some(entry) =
                self.latest_network_entry(page_id.as_str(), url_contains.as_str())?
            {
                if entry.response.is_some() {
                    return serialize_to_browser_value(
                        &entry,
                        "failed to serialize network response",
                    );
                }
            }
            sleep(NETWORK_POLL_INTERVAL).await;
        }

        if let Some(entry) =
            self.latest_failed_network_entry(page_id.as_str(), url_contains.as_str())?
        {
            let reason = entry
                .failure_text
                .unwrap_or_else(|| "request failed before a response was received".to_string());
            bail!("request matching `{url_contains}` failed: {reason}");
        }

        bail!("timed out waiting for response url to contain `{url_contains}`")
    }

    pub(super) async fn network_response_body(
        &mut self,
        url_contains: String,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        self.ensure_network_tracking_for_page(page.clone()).await?;
        let page_id = page.target_id().as_ref().to_string();
        let deadline = Instant::now() + NETWORK_WAIT_TIMEOUT;

        while Instant::now() < deadline {
            if let Some(entry) =
                self.latest_completed_network_entry(page_id.as_str(), url_contains.as_str())?
            {
                let response = page
                    .execute(GetResponseBodyParams::new(entry.request_id.clone()))
                    .await
                    .with_context(|| {
                        format!(
                            "failed to read response body for request matching `{url_contains}`"
                        )
                    })?;
                let body =
                    decode_response_body(response.result.body, response.result.base64_encoded)?;
                return Ok(BrowserValue::String(body));
            }
            sleep(NETWORK_POLL_INTERVAL).await;
        }

        if let Some(entry) =
            self.latest_failed_network_entry(page_id.as_str(), url_contains.as_str())?
        {
            let reason = entry.failure_text.unwrap_or_else(|| {
                "request failed before the response body was available".to_string()
            });
            bail!("request matching `{url_contains}` failed: {reason}");
        }

        bail!("timed out waiting for response body for url containing `{url_contains}`")
    }

    fn network_page_entries(&self, page_id: &str) -> Result<Vec<NetworkEntry>> {
        let state = self
            .network_state
            .lock()
            .map_err(|_| anyhow!("network log is not available"))?;
        Ok(state.page_entries(page_id))
    }

    fn latest_network_entry(&self, page_id: &str, fragment: &str) -> Result<Option<NetworkEntry>> {
        let state = self
            .network_state
            .lock()
            .map_err(|_| anyhow!("network log is not available"))?;
        Ok(state.latest_matching_entry(page_id, fragment))
    }

    fn latest_completed_network_entry(
        &self,
        page_id: &str,
        fragment: &str,
    ) -> Result<Option<NetworkEntry>> {
        let state = self
            .network_state
            .lock()
            .map_err(|_| anyhow!("network log is not available"))?;
        Ok(state.latest_completed_entry(page_id, fragment))
    }

    fn latest_failed_network_entry(
        &self,
        page_id: &str,
        fragment: &str,
    ) -> Result<Option<NetworkEntry>> {
        let state = self
            .network_state
            .lock()
            .map_err(|_| anyhow!("network log is not available"))?;
        Ok(state.latest_failed_entry(page_id, fragment))
    }
}

impl NetworkState {
    fn record_request(&mut self, page_id: &str, event: EventRequestWillBeSent) {
        let request_id = event.request_id.as_ref().to_string();
        let key = network_entry_key(page_id, request_id.as_str());
        let request = event.request;

        let entry = NetworkEntry {
            page_id: page_id.to_string(),
            request_id,
            request: NetworkRequestInfo {
                url: request.url,
                method: request.method,
                document_url: event.document_url,
                headers: request.headers.inner().clone(),
                resource_type: event
                    .r#type
                    .map(|resource_type| resource_type.as_ref().to_string()),
                has_post_data: request.has_post_data.unwrap_or(false),
                timestamp: *event.timestamp.inner(),
            },
            response: None,
            finished: false,
            failed: false,
            failure_text: None,
        };

        if let Some(index) = self.entry_index.get(key.as_str()).copied() {
            self.entries[index] = entry;
        } else {
            let index = self.entries.len();
            self.entries.push(entry);
            self.entry_index.insert(key, index);
        }
    }

    fn record_response(&mut self, page_id: &str, event: EventResponseReceived) {
        let request_id = event.request_id.as_ref().to_string();
        let response = event.response;
        let page_entry = self.entry_mut(page_id, request_id.as_str(), response.url.clone());

        page_entry.response = Some(NetworkResponseInfo {
            url: response.url,
            status: response.status,
            status_text: response.status_text,
            mime_type: response.mime_type,
            headers: response.headers.inner().clone(),
            resource_type: event.r#type.as_ref().to_string(),
            remote_ip_address: response.remote_ip_address,
            from_disk_cache: response.from_disk_cache.unwrap_or(false),
            from_service_worker: response.from_service_worker.unwrap_or(false),
            encoded_data_length: response.encoded_data_length,
            timestamp: *event.timestamp.inner(),
        });
    }

    fn mark_finished(&mut self, page_id: &str, event: EventLoadingFinished) {
        let request_id = event.request_id.as_ref().to_string();
        let page_entry = self.entry_mut(page_id, request_id.as_str(), String::new());
        page_entry.finished = true;
        if let Some(response) = page_entry.response.as_mut() {
            response.encoded_data_length = event.encoded_data_length;
        }
    }

    fn mark_failed(&mut self, page_id: &str, event: EventLoadingFailed) {
        let request_id = event.request_id.as_ref().to_string();
        let page_entry = self.entry_mut(page_id, request_id.as_str(), String::new());
        page_entry.failed = true;
        page_entry.failure_text = Some(event.error_text);
    }

    fn page_entries(&self, page_id: &str) -> Vec<NetworkEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.page_id == page_id)
            .cloned()
            .collect()
    }

    fn latest_matching_entry(&self, page_id: &str, fragment: &str) -> Option<NetworkEntry> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.page_id == page_id && entry_matches(entry, fragment))
            .cloned()
    }

    fn latest_completed_entry(&self, page_id: &str, fragment: &str) -> Option<NetworkEntry> {
        self.entries
            .iter()
            .rev()
            .find(|entry| {
                entry.page_id == page_id
                    && entry.finished
                    && !entry.failed
                    && entry.response.is_some()
                    && entry_matches(entry, fragment)
            })
            .cloned()
    }

    fn latest_failed_entry(&self, page_id: &str, fragment: &str) -> Option<NetworkEntry> {
        self.entries
            .iter()
            .rev()
            .find(|entry| {
                entry.page_id == page_id && entry.failed && entry_matches(entry, fragment)
            })
            .cloned()
    }

    fn entry_mut(
        &mut self,
        page_id: &str,
        request_id: &str,
        url_hint: String,
    ) -> &mut NetworkEntry {
        let key = network_entry_key(page_id, request_id);
        if let Some(index) = self.entry_index.get(key.as_str()).copied() {
            return &mut self.entries[index];
        }

        let index = self.entries.len();
        self.entries.push(NetworkEntry {
            page_id: page_id.to_string(),
            request_id: request_id.to_string(),
            request: placeholder_request(url_hint),
            response: None,
            finished: false,
            failed: false,
            failure_text: None,
        });
        self.entry_index.insert(key, index);
        &mut self.entries[index]
    }
}

fn decode_response_body(body: String, base64_encoded: bool) -> Result<String> {
    if !base64_encoded {
        return Ok(body);
    }

    let decoded = STANDARD
        .decode(body.as_bytes())
        .context("failed to decode base64 response body")?;

    match String::from_utf8(decoded) {
        Ok(text) => Ok(text),
        Err(_) => Ok(body),
    }
}

fn placeholder_request(url: String) -> NetworkRequestInfo {
    NetworkRequestInfo {
        url,
        method: String::new(),
        document_url: String::new(),
        headers: serde_json::Value::Null,
        resource_type: None,
        has_post_data: false,
        timestamp: 0.0,
    }
}

fn network_entry_key(page_id: &str, request_id: &str) -> String {
    format!("{page_id}:{request_id}")
}

fn entry_matches(entry: &NetworkEntry, fragment: &str) -> bool {
    fragment.is_empty()
        || entry.request.url.contains(fragment)
        || entry
            .response
            .as_ref()
            .map(|response| response.url.contains(fragment))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(
        page_id: &str,
        request_id: &str,
        request_url: &str,
        response_url: Option<&str>,
        finished: bool,
        failed: bool,
    ) -> NetworkEntry {
        NetworkEntry {
            page_id: page_id.to_string(),
            request_id: request_id.to_string(),
            request: NetworkRequestInfo {
                url: request_url.to_string(),
                method: "GET".to_string(),
                document_url: request_url.to_string(),
                headers: serde_json::json!({}),
                resource_type: Some("XHR".to_string()),
                has_post_data: false,
                timestamp: 1.0,
            },
            response: response_url.map(|response_url| NetworkResponseInfo {
                url: response_url.to_string(),
                status: 200,
                status_text: "OK".to_string(),
                mime_type: "application/json".to_string(),
                headers: serde_json::json!({}),
                resource_type: "XHR".to_string(),
                remote_ip_address: None,
                from_disk_cache: false,
                from_service_worker: false,
                encoded_data_length: 128.0,
                timestamp: 2.0,
            }),
            finished,
            failed,
            failure_text: failed.then(|| "net::ERR_FAILED".to_string()),
        }
    }

    #[test]
    fn latest_completed_entry_prefers_newest_match() {
        let mut state = NetworkState::default();
        state.entries = vec![
            make_entry(
                "page-1",
                "req-1",
                "https://example.com/api/search?q=old",
                Some("https://example.com/api/search?q=old"),
                true,
                false,
            ),
            make_entry(
                "page-1",
                "req-2",
                "https://example.com/api/search?q=new",
                Some("https://example.com/api/search?q=new"),
                true,
                false,
            ),
        ];

        let matched = state
            .latest_completed_entry("page-1", "search")
            .expect("should find a completed response");

        assert_eq!(matched.request_id, "req-2");
    }

    #[test]
    fn latest_matching_entry_checks_response_url_too() {
        let mut state = NetworkState::default();
        state.entries = vec![make_entry(
            "page-1",
            "req-1",
            "https://example.com/redirect",
            Some("https://api.example.com/final"),
            false,
            false,
        )];

        let matched = state
            .latest_matching_entry("page-1", "api.example.com")
            .expect("should match response url");

        assert_eq!(matched.request_id, "req-1");
    }

    #[test]
    fn decode_response_body_supports_plain_text() {
        let decoded =
            decode_response_body("hello".to_string(), false).expect("plain text should decode");
        assert_eq!(decoded, "hello");
    }
}
