use chromiumoxide::cdp::browser_protocol::network::{
    EnableParams, EventLoadingFailed, EventLoadingFinished, EventRequestWillBeSent,
    EventResponseReceived,
};

use super::{
    actor::BrowserActor,
    types::{BrowserValue, NetworkEntry, NetworkRequestInfo, NetworkResponseInfo, NetworkState},
    util::serialize_to_browser_value,
    *,
};

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

    pub(super) async fn network_log(
        &mut self,
        url_contains: Option<String>,
    ) -> Result<BrowserValue> {
        let page = self.ensure_active_page().await?;
        self.ensure_network_tracking_for_page(page.clone()).await?;
        let page_id = page.target_id().as_ref().to_string();
        let entries = self.network_page_entries(page_id.as_str(), url_contains.as_deref())?;
        serialize_to_browser_value(&entries, "failed to serialize network log")
    }

    fn network_page_entries(
        &self,
        page_id: &str,
        url_contains: Option<&str>,
    ) -> Result<Vec<NetworkEntry>> {
        let state = self
            .network_state
            .lock()
            .map_err(|_| anyhow!("network log is not available"))?;
        Ok(state.page_entries(page_id, url_contains))
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

    fn page_entries(&self, page_id: &str, url_contains: Option<&str>) -> Vec<NetworkEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                entry.page_id == page_id
                    && url_contains
                        .map(|fragment| entry_matches(entry, fragment))
                        .unwrap_or(true)
            })
            .cloned()
            .collect()
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
            finished: false,
            failed: false,
            failure_text: None,
        }
    }

    #[test]
    fn page_entries_returns_page_entries_in_recorded_order() {
        let mut state = NetworkState::default();
        state.entries = vec![
            make_entry(
                "page-1",
                "req-1",
                "https://example.com/api/search?q=old",
                Some("https://example.com/api/search?q=old"),
            ),
            make_entry(
                "page-1",
                "req-2",
                "https://example.com/api/search?q=new",
                Some("https://example.com/api/search?q=new"),
            ),
            make_entry(
                "page-2",
                "req-3",
                "https://example.com/api/search?q=other-page",
                Some("https://example.com/api/search?q=other-page"),
            ),
        ];

        let entries = state.page_entries("page-1", None);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].request_id, "req-1");
        assert_eq!(entries[1].request_id, "req-2");
    }

    #[test]
    fn page_entries_filter_checks_response_url_too() {
        let mut state = NetworkState::default();
        state.entries = vec![make_entry(
            "page-1",
            "req-1",
            "https://example.com/redirect",
            Some("https://api.example.com/final"),
        )];

        let entries = state.page_entries("page-1", Some("api.example.com"));

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].request_id, "req-1");
    }
}
