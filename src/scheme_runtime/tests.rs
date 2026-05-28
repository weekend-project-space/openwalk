use std::{
    env,
    ffi::OsString,
    fs, process,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;
use crate::workspace::GlobalHome;
use scheme4r::eval::Engine;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "openwalk-scheme-test-{}-{timestamp}",
            process::id()
        ));
        fs::create_dir_all(&path).expect("test temp dir should be created");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = env::var_os(key);
        env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            env::set_var(self.key, previous);
        } else {
            env::remove_var(self.key);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_runs_plain_scheme_without_browser() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("math.scm");
    fs::write(&script_path, "(define (main args) (+ 1 2 3))").expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect("script should execute");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert_eq!(expect_test_number(&result), 6);
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_exposes_openwalk_args() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("args.scm");
    fs::write(&script_path, "(define (main args) (car args))").expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(
        &script_path,
        &[String::from("hello")],
        browser.client(),
        None,
    )
    .await
    .expect("script should execute");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert_eq!(expect_test_string(&result), "hello");
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_calls_main_with_cli_args() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("main.scm");
    fs::write(
        &script_path,
        "(define (main args) (if (null? args) \"empty\" (car args)))",
    )
    .expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(
        &script_path,
        &[String::from("from-cli")],
        browser.client(),
        None,
    )
    .await
    .expect("script should execute");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert_eq!(expect_test_string(&result), "from-cli");
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_falls_back_to_top_level_value_without_main() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("fallback.scm");
    fs::write(&script_path, "(+ 40 2)").expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect("script should execute");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert_eq!(expect_test_number(&result), 42);
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_supports_domain_style_builtins() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("domain-names.scm");
    fs::write(
        &script_path,
        "(define (main args) (if (time-sleep 0) \"domain-ok\" \"domain-bad\"))",
    )
    .expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect("script should execute");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert_eq!(expect_test_string(&result), "domain-ok");
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_rejects_legacy_browser_builtin_names() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("legacy-name.scm");
    fs::write(&script_path, "(define (main args) browser-goto)").expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let error = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect_err("legacy names should no longer be registered");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert!(error
        .to_string()
        .contains("undefined variable: browser-goto"));
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_rejects_legacy_browser_tab_builtin_names() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("legacy-tab-name.scm");
    fs::write(&script_path, "(define (main args) browser-tabs)").expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let error = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect_err("legacy tab names should no longer be registered");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert!(error
        .to_string()
        .contains("undefined variable: browser-tabs"));
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_rejects_pre_refactor_scheme_names() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("legacy-domain-name.scm");
    fs::write(&script_path, "(define (main args) input-click)").expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let error = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect_err("old scheme names should no longer be registered");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert!(error
        .to_string()
        .contains("undefined variable: input-click"));
}

#[test]
fn builtin_tool_metadata_exposes_browser_open() {
    let metadata = builtin_tools::builtin_tool_metadata("browser-open")
        .expect("browser-open metadata should exist");

    assert_eq!(metadata.name, "browser-open");
    assert_eq!(metadata.description, "打开浏览器并导航到指定 URL。");
    assert_eq!(metadata.returns.return_type, "string");
    assert_eq!(metadata.args.len(), 1);
    assert_eq!(metadata.args[0].name, "url");
}

#[test]
fn builtin_tool_metadata_exposes_browser_list() {
    let metadata = builtin_tools::builtin_tool_metadata("browser-list")
        .expect("browser-list metadata should exist");

    assert_eq!(metadata.name, "browser-list");
    assert_eq!(metadata.description, "列出当前环境已记录的浏览器会话名称。");
    assert_eq!(metadata.returns.return_type, "array");
    assert!(metadata.args.is_empty());
    assert!(metadata.read_only);
}

#[test]
fn browser_list_returns_scheme_vector() {
    let _env_guard = ENV_LOCK.lock().expect("env lock should be acquired");
    let sandbox = TestDir::new();
    let global_home_root = sandbox.path.join("global-home");
    let _openwalk_home = EnvVarGuard::set(
        "OPENWALK_HOME",
        global_home_root.to_str().expect("utf8 path"),
    );

    let global_home = GlobalHome::discover().expect("global home should resolve");
    global_home.init().expect("global home should initialize");

    for session_name in ["qa", "default"] {
        let session_dir = global_home.browser_session_dir(session_name);
        fs::create_dir_all(&session_dir).expect("session dir should be created");
        fs::write(session_dir.join("session.json"), "{}")
            .expect("session manifest should be written");
    }

    let value = browser_list(&Engine::new(Environment::standard()), &[])
        .expect("browser-list should return a value");

    let Value::Vector(items) = value else {
        panic!("browser-list should return a vector");
    };
    let items = items.borrow();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items.iter().map(expect_test_string).collect::<Vec<_>>(),
        vec!["default".to_string(), "qa".to_string()]
    );
}

#[test]
fn builtin_tool_metadata_exposes_page_goto() {
    let metadata =
        builtin_tools::builtin_tool_metadata("page-goto").expect("page-goto metadata should exist");

    assert_eq!(metadata.name, "page-goto");
    assert_eq!(metadata.description, "在当前活动标签页导航到指定 URL。");
    assert_eq!(metadata.returns.return_type, "string");
    assert_eq!(metadata.args.len(), 1);
    assert_eq!(metadata.args[0].name, "url");
}

#[test]
fn builtin_tool_metadata_exposes_element_upload() {
    let metadata = builtin_tools::builtin_tool_metadata("element-upload")
        .expect("element-upload metadata should exist");

    assert_eq!(metadata.name, "element-upload");
    assert_eq!(metadata.returns.return_type, "array");
    assert_eq!(metadata.args.len(), 2);
    assert_eq!(metadata.args[0].name, "selector");
    assert_eq!(metadata.args[1].name, "file");
}

#[test]
fn builtin_tool_metadata_exposes_page_snapshot() {
    let metadata = builtin_tools::builtin_tool_metadata("page-snapshot")
        .expect("page-snapshot metadata should exist");

    assert_eq!(metadata.name, "page-snapshot");
    assert_eq!(metadata.returns.return_type, "object");
    assert!(metadata.args.is_empty());
    assert!(metadata.read_only);
}

#[test]
fn builtin_tool_metadata_exposes_console() {
    let metadata =
        builtin_tools::builtin_tool_metadata("console").expect("console metadata should exist");

    assert_eq!(metadata.name, "console");
    assert_eq!(metadata.returns.return_type, "array");
    assert_eq!(metadata.args.len(), 1);
    assert_eq!(metadata.args[0].name, "min-level");
    assert!(metadata.read_only);
}

#[test]
fn browser_upload_requires_at_least_one_file() {
    let engine = Engine::new(Environment::standard());
    let error = browser_upload(&engine, &[Value::string("input[type=file]")])
        .expect_err("element-upload should reject a missing file argument");

    assert!(error
        .to_string()
        .contains("`element-upload` expects between 2 and"));
}

#[test]
fn browser_console_rejects_extra_arguments() {
    let engine = Engine::new(Environment::standard());
    let error = browser_console(&engine, &[Value::string("warn"), Value::string("extra")])
        .expect_err("console should only allow an optional min-level");

    assert!(error
        .to_string()
        .contains("`console` expects between 0 and 1 argument(s)"));
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_builtin_supports_cli_number_arguments() {
    let browser = crate::browser::BrowserService::spawn();
    let result = execute_builtin("time-sleep", &[String::from("0")], browser.client(), None)
        .await
        .expect("builtin should execute");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert!(expect_test_bool(&result));
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_builtin_tab_new_requires_browser_open_first() {
    let browser = crate::browser::BrowserService::spawn();
    let error = execute_builtin("tab-new", &[], browser.client(), None)
        .await
        .expect_err("tab-new should require browser-open first");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    let message = error.to_string();
    assert!(message.contains("tab-new"));
    assert!(message.contains("browser-open"));
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_with_exception_handler_catches_host_tool_error() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("with-exception-handler.scm");
    fs::write(
        &script_path,
        r#"
            (define (main args)
              (with-exception-handler
                (lambda (e)
                  (if (error-object? e)
                      (error-object-message e)
                      "unexpected"))
                (lambda ()
                  (tab-new))))
            "#,
    )
    .expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect("script should catch host tool failure");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    let message = expect_test_string(&result);
    assert!(message.contains("tab-new"));
    assert!(message.contains("browser-open"));
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_exposes_openwalk_session_name() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("session-name.scm");
    fs::write(&script_path, "(define (main args) openwalk-session-name)")
        .expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(&script_path, &[], browser.client(), Some("qa".to_string()))
        .await
        .expect("script should expose session name");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert_eq!(expect_test_string(&result), "qa");
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_exposes_false_when_session_name_is_missing() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("session-name-missing.scm");
    fs::write(&script_path, "(define (main args) openwalk-session-name)")
        .expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect("script should expose a falsey session value");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    let Value::Boolean(value) = result else {
        panic!("openwalk-session-name should be #f without a named session");
    };
    assert!(!value);
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_exposes_openwalk_script_meta_as_alist() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("script-meta.scm");
    fs::write(
        &script_path,
        r#"
            #| @meta
            {
              "name": "demo-tool",
              "description": "demo description",
              "args": [],
              "returns": {
                "type": "string",
                "description": "demo return"
              },
              "examples": ["openwalk run demo-tool"],
              "domains": [],
              "readOnly": true,
              "requiresLogin": false,
              "tags": ["demo"]
            }
            |#

            (define (main args)
              (cdr (assoc "name" openwalk-script-meta)))
            "#,
    )
    .expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect("script should expose script metadata");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    assert_eq!(expect_test_string(&result), "demo-tool");
}

#[tokio::test(flavor = "multi_thread")]
async fn execute_script_exposes_false_when_script_meta_is_missing() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("script-meta-missing.scm");
    fs::write(&script_path, "(define (main args) openwalk-script-meta)")
        .expect("script should be written");

    let browser = crate::browser::BrowserService::spawn();
    let result = execute_script(&script_path, &[], browser.client(), None)
        .await
        .expect("script should expose a falsey metadata value");
    browser
        .shutdown()
        .await
        .expect("browser service should stop");

    let Value::Boolean(value) = result else {
        panic!("openwalk-script-meta should be #f without a meta header");
    };
    assert!(!value);
}

#[test]
fn scheme_builtin_list_is_large_enough() {
    assert!(SCHEME_BUILTINS.len() >= 60);
}

#[test]
fn scheme_builtin_list_contains_tab_helpers() {
    assert!(SCHEME_BUILTINS.contains(&"tab-list"));
    assert!(SCHEME_BUILTINS.contains(&"tab-new"));
    assert!(SCHEME_BUILTINS.contains(&"tab-select"));
    assert!(SCHEME_BUILTINS.contains(&"tab-close"));
}

#[test]
fn scheme_builtin_list_contains_network_helpers() {
    assert!(SCHEME_BUILTINS.contains(&"network-list"));
    assert!(SCHEME_BUILTINS.contains(&"network-wait-response"));
    assert!(SCHEME_BUILTINS.contains(&"network-response-body"));
}

#[test]
fn scheme_builtin_list_contains_refactored_domain_helpers() {
    assert!(SCHEME_BUILTINS.contains(&"browser-list"));
    assert!(SCHEME_BUILTINS.contains(&"page-goto"));
    assert!(SCHEME_BUILTINS.contains(&"element-click"));
    assert!(SCHEME_BUILTINS.contains(&"element-upload"));
    assert!(SCHEME_BUILTINS.contains(&"element-drag"));
    assert!(SCHEME_BUILTINS.contains(&"keyboard-press"));
    assert!(SCHEME_BUILTINS.contains(&"mouse-click"));
    assert!(SCHEME_BUILTINS.contains(&"touch-tap"));
    assert!(SCHEME_BUILTINS.contains(&"js-eval"));
    assert!(SCHEME_BUILTINS.contains(&"js-wait"));
    assert!(SCHEME_BUILTINS.contains(&"time-sleep"));
    assert!(SCHEME_BUILTINS.contains(&"device-viewport"));
    assert!(SCHEME_BUILTINS.contains(&"cookie-list"));
    assert!(SCHEME_BUILTINS.contains(&"localstorage-get"));
    assert!(SCHEME_BUILTINS.contains(&"sessionstorage-get"));
}

#[test]
fn scheme_builtin_list_contains_snapshot_helpers() {
    assert!(SCHEME_BUILTINS.contains(&"page-snapshot"));
}

#[test]
fn scheme_builtin_list_contains_console_helpers() {
    assert!(SCHEME_BUILTINS.contains(&"console"));
    assert!(SCHEME_BUILTINS.contains(&"console-clear"));
}

#[test]
fn scheme_builtin_list_contains_inspect_helpers() {
    assert!(SCHEME_BUILTINS.contains(&"inspect-info"));
    assert!(SCHEME_BUILTINS.contains(&"inspect-highlight"));
    assert!(SCHEME_BUILTINS.contains(&"inspect-hide-highlight"));
    assert!(SCHEME_BUILTINS.contains(&"inspect-pick"));
}

#[test]
fn scheme_builtin_list_contains_tracing_helpers() {
    assert!(SCHEME_BUILTINS.contains(&"tracing-start"));
    assert!(SCHEME_BUILTINS.contains(&"tracing-stop"));
}

fn expect_test_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_plain_string(),
        other => panic!("expected string, got {other}"),
    }
}

fn expect_test_number(value: &Value) -> i64 {
    match value {
        Value::Number(number) => *number,
        other => panic!("expected number, got {other}"),
    }
}

fn expect_test_bool(value: &Value) -> bool {
    match value {
        Value::Boolean(boolean) => *boolean,
        other => panic!("expected boolean, got {other}"),
    }
}
