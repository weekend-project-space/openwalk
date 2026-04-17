use base64::{engine::general_purpose::STANDARD, Engine as _};
use chromiumoxide::cdp::browser_protocol::{
    io::{CloseParams as IoCloseParams, ReadParams as IoReadParams},
    tracing::{
        EndParams as TracingEndParams, EventTracingComplete, StartParams as TracingStartParams,
        StartTransferMode, StreamCompression, StreamFormat, TraceConfig, TraceConfigRecordMode,
    },
};
use tokio::time::timeout;

use super::super::{
    actor::BrowserActor,
    types::{BrowserValue, TraceSession, TraceStartInfo, TraceStopInfo},
    util::serialize_to_browser_value,
    *,
};

const TRACE_READ_CHUNK_SIZE: i64 = 512 * 1024;
const TRACE_STOP_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_TRACE_CATEGORIES: &[&str] = &[
    "devtools.timeline",
    "blink.user_timing",
    "v8",
    "disabled-by-default-devtools.screenshot",
];

impl BrowserActor {
    pub(in crate::browser) async fn tracing_start(
        &mut self,
        categories: Option<String>,
    ) -> Result<BrowserValue> {
        if self.trace_session.is_some() {
            bail!("tracing is already active, call `tracing-stop` before starting a new session");
        }

        let page = self.ensure_active_page().await?;
        let page_id = page.target_id().as_ref().to_string();
        let categories = normalize_trace_categories(categories.as_deref());

        page.execute(
            TracingStartParams::builder()
                .transfer_mode(StartTransferMode::ReturnAsStream)
                .stream_format(StreamFormat::Json)
                .stream_compression(StreamCompression::None)
                .trace_config(
                    TraceConfig::builder()
                        .record_mode(TraceConfigRecordMode::RecordContinuously)
                        .included_categories(categories.clone())
                        .build(),
                )
                .build(),
        )
        .await
        .context("failed to start tracing")?;

        self.trace_session = Some(TraceSession {
            page_id: page_id.clone(),
            categories: categories.clone(),
        });

        let info = TraceStartInfo {
            page_id,
            categories,
        };
        serialize_to_browser_value(&info, "failed to serialize trace start info")
    }

    pub(in crate::browser) async fn tracing_stop(&mut self, path: String) -> Result<BrowserValue> {
        let session = self
            .trace_session
            .clone()
            .ok_or_else(|| anyhow!("no active tracing session. Call `tracing-start` first"))?;
        let page = self
            .pages
            .iter()
            .find(|page| page.target_id().as_ref() == session.page_id.as_str())
            .cloned()
            .ok_or_else(|| anyhow!("the page used for tracing is no longer available"))?;

        let mut tracing_complete_events = page
            .event_listener::<EventTracingComplete>()
            .await
            .context("failed to subscribe to tracing-complete events")?;
        page.execute(TracingEndParams::default())
            .await
            .context("failed to stop tracing")?;

        let complete_event = timeout(TRACE_STOP_TIMEOUT, async {
            tracing_complete_events
                .next()
                .await
                .ok_or_else(|| anyhow!("tracing completed without a completion event"))
                .map(|event| event.as_ref().clone())
        })
        .await
        .context("timed out waiting for tracing data to flush")??;

        let stream = complete_event
            .stream
            .clone()
            .ok_or_else(|| anyhow!("tracing completed without a stream payload"))?;
        let trace_bytes = read_cdp_stream(&page, stream).await?;
        tokio::fs::write(path.as_str(), &trace_bytes)
            .await
            .with_context(|| format!("failed to write trace file `{path}`"))?;

        self.trace_session = None;

        let info = TraceStopInfo {
            page_id: session.page_id,
            path,
            categories: session.categories,
            bytes_written: trace_bytes.len(),
            data_loss_occurred: complete_event.data_loss_occurred,
            trace_format: complete_event
                .trace_format
                .map(|format: StreamFormat| format.as_ref().to_string()),
            stream_compression: complete_event
                .stream_compression
                .map(|compression: StreamCompression| compression.as_ref().to_string()),
        };
        serialize_to_browser_value(&info, "failed to serialize trace stop info")
    }
}

async fn read_cdp_stream(
    page: &Page,
    handle: chromiumoxide::cdp::browser_protocol::io::StreamHandle,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();

    loop {
        let chunk = page
            .execute(
                IoReadParams::builder()
                    .handle(handle.clone())
                    .size(TRACE_READ_CHUNK_SIZE)
                    .build()
                    .expect("io read params should build"),
            )
            .await
            .context("failed to read trace stream chunk")?;
        output.extend(decode_stream_chunk(
            chunk.result.data,
            chunk.result.base64_encoded.unwrap_or(false),
        )?);
        if chunk.result.eof {
            break;
        }
    }

    page.execute(IoCloseParams::new(handle))
        .await
        .context("failed to close trace stream")?;

    Ok(output)
}

fn decode_stream_chunk(data: String, is_base64: bool) -> Result<Vec<u8>> {
    if is_base64 {
        STANDARD
            .decode(data)
            .context("failed to decode base64 trace stream chunk")
    } else {
        Ok(data.into_bytes())
    }
}

fn normalize_trace_categories(raw: Option<&str>) -> Vec<String> {
    let source = raw.unwrap_or_default().trim();
    if source.is_empty() {
        return DEFAULT_TRACE_CATEGORIES
            .iter()
            .map(|value| (*value).to_string())
            .collect();
    }

    let mut categories = Vec::new();
    for part in source
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        if !categories.iter().any(|existing| existing == part) {
            categories.push(part.to_string());
        }
    }

    if categories.is_empty() {
        DEFAULT_TRACE_CATEGORIES
            .iter()
            .map(|value| (*value).to_string())
            .collect()
    } else {
        categories
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trace_categories_uses_defaults() {
        let categories = normalize_trace_categories(None);

        assert!(categories.contains(&"devtools.timeline".to_string()));
        assert!(categories.contains(&"v8".to_string()));
    }

    #[test]
    fn normalize_trace_categories_trims_and_dedupes() {
        let categories = normalize_trace_categories(Some(" a, b ,a ,, c "));

        assert_eq!(categories, vec!["a", "b", "c"]);
    }

    #[test]
    fn decode_stream_chunk_supports_plain_and_base64() {
        assert_eq!(
            decode_stream_chunk("hello".to_string(), false).unwrap(),
            b"hello".to_vec()
        );
        assert_eq!(
            decode_stream_chunk("aGVsbG8=".to_string(), true).unwrap(),
            b"hello".to_vec()
        );
    }
}
