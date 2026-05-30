use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use tokio::runtime::Handle;

use crate::{
    browser::{
        attach_browser_session_with_options, browser_session_daemon_port,
        ensure_browser_session_with_options, record_browser_session_daemon_port, BrowserService,
        BrowserSessionLaunchOptions,
    },
    runtime_args::BrowserLaunchOptions,
    scheme_runtime, value_codec,
    workspace::GlobalHome,
};

const DAEMON_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(5);
const DAEMON_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum DaemonRequest {
    Ping,
    Execute {
        tool: String,
        args: Vec<String>,
        launch: DaemonLaunchOptions,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonLaunchOptions {
    headless: Option<bool>,
    profile: Option<PathBuf>,
    create_session_if_missing: bool,
}

impl From<&BrowserLaunchOptions> for DaemonLaunchOptions {
    fn from(value: &BrowserLaunchOptions) -> Self {
        Self {
            headless: value.headless,
            profile: value.profile.clone(),
            create_session_if_missing: value.create_session_if_missing,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DaemonResponse {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl DaemonResponse {
    fn ok(result: JsonValue) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    fn err(error: String) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

struct SessionDaemonState {
    browser: Option<BrowserService>,
}

pub async fn run_session_daemon(session_name: String, port: u16) -> Result<()> {
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || run_session_daemon_blocking(session_name, port, handle))
        .await
        .context("session daemon task failed to join")?
}

pub async fn execute_session_builtin(
    global_home: &GlobalHome,
    session_name: &str,
    tool: &str,
    args: &[String],
    launch_options: &BrowserLaunchOptions,
) -> Result<JsonValue> {
    let port = ensure_session_daemon(global_home, session_name).await?;
    let response = send_daemon_request(
        port,
        &DaemonRequest::Execute {
            tool: tool.to_string(),
            args: args.to_vec(),
            launch: DaemonLaunchOptions::from(launch_options),
        },
    )?;

    if !response.ok {
        return Err(anyhow!(
            "{}",
            response
                .error
                .unwrap_or_else(|| "session daemon request failed".to_string())
        ));
    }

    if tool != "browser-close" {
        record_browser_session_daemon_port(global_home, session_name, port)?;
    }

    response
        .result
        .ok_or_else(|| anyhow!("session daemon returned an empty response"))
}

async fn ensure_session_daemon(global_home: &GlobalHome, session_name: &str) -> Result<u16> {
    if let Some(port) = browser_session_daemon_port(global_home, session_name)? {
        if daemon_is_alive(port) {
            return Ok(port);
        }
    }

    let port = pick_free_port()?;
    spawn_session_daemon(session_name, port)?;
    wait_for_session_daemon(port).await?;
    Ok(port)
}

fn run_session_daemon_blocking(session_name: String, port: u16, handle: Handle) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("failed to bind session daemon on 127.0.0.1:{port}"))?;
    let global_home = GlobalHome::discover().context("failed to discover openwalk home")?;
    let mut state = SessionDaemonState { browser: None };

    for stream in listener.incoming() {
        let mut stream = stream.context("failed to accept session daemon connection")?;
        let request = read_daemon_request(&mut stream);
        let mut should_stop = false;
        let response = match request {
            Ok(DaemonRequest::Ping) => DaemonResponse::ok(json!({"status": "ok"})),
            Ok(DaemonRequest::Execute { tool, args, launch }) => {
                let result = handle.block_on(execute_daemon_builtin(
                    &global_home,
                    session_name.as_str(),
                    &mut state,
                    tool.as_str(),
                    args.as_slice(),
                    &launch,
                ));
                if tool == "browser-close" && result.is_ok() {
                    should_stop = true;
                }
                match result {
                    Ok(result) => DaemonResponse::ok(result),
                    Err(err) => DaemonResponse::err(format!("{err:#}")),
                }
            }
            Err(err) => DaemonResponse::err(format!("{err:#}")),
        };

        let _ = write_daemon_response(&mut stream, &response);
        if should_stop {
            break;
        }
    }

    if let Some(browser) = state.browser.take() {
        let _ = handle.block_on(browser.shutdown());
    }

    Ok(())
}

async fn execute_daemon_builtin(
    global_home: &GlobalHome,
    session_name: &str,
    state: &mut SessionDaemonState,
    tool: &str,
    args: &[String],
    launch: &DaemonLaunchOptions,
) -> Result<JsonValue> {
    if state.browser.is_none() {
        state.browser =
            Some(create_session_browser_service(global_home, session_name, launch).await?);
    }

    let browser = state
        .browser
        .as_ref()
        .ok_or_else(|| anyhow!("session browser service is not available"))?;
    let result = scheme_runtime::execute_builtin(
        tool,
        args,
        browser.client(),
        Some(session_name.to_string()),
    )
    .await?;
    let payload = value_codec::scheme_value_to_json(&result);

    if tool == "browser-close" {
        if let Some(browser) = state.browser.take() {
            browser.shutdown().await?;
        }
    }

    Ok(payload)
}

async fn create_session_browser_service(
    global_home: &GlobalHome,
    session_name: &str,
    launch: &DaemonLaunchOptions,
) -> Result<BrowserService> {
    let session_options = BrowserSessionLaunchOptions {
        requested_headless: launch.headless,
        requested_profile_dir: launch.profile.clone(),
    };
    let handle = if launch.create_session_if_missing {
        ensure_browser_session_with_options(global_home, session_name, session_options).await?
    } else {
        attach_browser_session_with_options(global_home, session_name, session_options).await?
    };
    Ok(BrowserService::attach_session(handle))
}

fn spawn_session_daemon(session_name: &str, port: u16) -> Result<()> {
    let executable = std::env::current_exe().context("failed to resolve current executable")?;
    let args = [
        "daemon".to_string(),
        "--session".to_string(),
        session_name.to_string(),
        "--port".to_string(),
        port.to_string(),
    ];

    spawn_detached_command(&executable, &args).context("failed to spawn session daemon")
}

#[cfg(unix)]
fn spawn_detached_command(executable: &std::path::Path, args: &[String]) -> Result<()> {
    let spawn_result = Command::new("setsid")
        .arg(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match spawn_result {
        Ok(_) => Ok(()),
        Err(_) => spawn_plain_command(executable, args),
    }
}

#[cfg(not(unix))]
fn spawn_detached_command(executable: &std::path::Path, args: &[String]) -> Result<()> {
    spawn_plain_command(executable, args)
}

fn spawn_plain_command(executable: &std::path::Path, args: &[String]) -> Result<()> {
    Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn session daemon process")?;
    Ok(())
}

async fn wait_for_session_daemon(port: u16) -> Result<()> {
    let deadline = std::time::Instant::now() + DAEMON_READY_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if daemon_is_alive(port) {
            return Ok(());
        }
        tokio::time::sleep(DAEMON_READY_POLL_INTERVAL).await;
    }
    Err(anyhow!(
        "timed out waiting for session daemon on port {port}"
    ))
}

fn daemon_is_alive(port: u16) -> bool {
    send_daemon_request(port, &DaemonRequest::Ping)
        .map(|response| response.ok)
        .unwrap_or(false)
}

fn send_daemon_request(port: u16, request: &DaemonRequest) -> Result<DaemonResponse> {
    let address = format!("127.0.0.1:{port}");
    let socket_addr = address
        .parse()
        .with_context(|| format!("invalid daemon address `{address}`"))?;
    let mut stream = TcpStream::connect_timeout(&socket_addr, DAEMON_CONNECT_TIMEOUT)
        .with_context(|| format!("failed to connect to session daemon at {address}"))?;
    let body = serde_json::to_vec(request).context("failed to encode session daemon request")?;
    stream
        .write_all(body.as_slice())
        .context("failed to write session daemon request")?;
    stream
        .shutdown(Shutdown::Write)
        .context("failed to finish session daemon request")?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("failed to read session daemon response")?;
    serde_json::from_str(response.as_str()).context("failed to decode session daemon response")
}

fn read_daemon_request(stream: &mut TcpStream) -> Result<DaemonRequest> {
    let mut body = String::new();
    stream
        .read_to_string(&mut body)
        .context("failed to read session daemon request")?;
    serde_json::from_str(body.as_str()).context("failed to decode session daemon request")
}

fn write_daemon_response(stream: &mut TcpStream, response: &DaemonResponse) -> Result<()> {
    let body = serde_json::to_vec(response).context("failed to encode session daemon response")?;
    stream
        .write_all(body.as_slice())
        .context("failed to write session daemon response")
}

fn pick_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to reserve a daemon port")?;
    let port = listener
        .local_addr()
        .context("failed to inspect reserved daemon port")?
        .port();
    drop(listener);
    Ok(port)
}
