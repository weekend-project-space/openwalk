use std::{
    env,
    ffi::OsString,
    fs,
    process::{self, Command},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::sync::Semaphore;

use super::*;
use crate::tool_hub::{OPENWALK_HUB_GIT_REF_ENV, OPENWALK_HUB_GIT_URL_ENV};

static CWD_LOCK: Semaphore = Semaphore::const_new(1);
static HUB_ENV_LOCK: Mutex<()> = Mutex::new(());

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let path = env::temp_dir().join(format!("openwalk-app-test-{}-{timestamp}", process::id()));
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

struct LocalHubRepo {
    _sandbox: TestDir,
    path: PathBuf,
}

impl LocalHubRepo {
    fn with_tool(name: &str, body: &str) -> Self {
        Self::with_tools(&[(name, body)])
    }

    fn with_tools(tools: &[(&str, &str)]) -> Self {
        let sandbox = TestDir::new();
        let path = sandbox.path.join("hub");
        for (name, body) in tools {
            fs::create_dir_all(path.join("tools").join(name))
                .expect("hub tool directory should be created");
            fs::write(path.join("tools").join(name).join("main.scm"), body)
                .expect("hub tool script should be written");
        }

        run_git(&path, &["init"]);
        run_git(&path, &["checkout", "-b", "main"]);
        run_git(&path, &["add", "."]);
        run_git(
            &path,
            &[
                "-c",
                "user.name=OpenWalk Tests",
                "-c",
                "user.email=tests@example.com",
                "commit",
                "-m",
                "initial hub fixture",
            ],
        );

        Self {
            _sandbox: sandbox,
            path,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

fn run_git(repo_dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .output()
        .expect("git command should launch in tests");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn install_test_hub_tool(name: &str, body: &str) -> (LocalHubRepo, EnvVarGuard, EnvVarGuard) {
    let repo = LocalHubRepo::with_tool(name, body);
    let url_guard = EnvVarGuard::set(
        OPENWALK_HUB_GIT_URL_ENV,
        repo.path().to_str().expect("utf8 path"),
    );
    let ref_guard = EnvVarGuard::set(OPENWALK_HUB_GIT_REF_ENV, "main");
    (repo, url_guard, ref_guard)
}

fn install_test_hub_tools(tools: &[(&str, &str)]) -> (LocalHubRepo, EnvVarGuard, EnvVarGuard) {
    let repo = LocalHubRepo::with_tools(tools);
    let url_guard = EnvVarGuard::set(
        OPENWALK_HUB_GIT_URL_ENV,
        repo.path().to_str().expect("utf8 path"),
    );
    let ref_guard = EnvVarGuard::set(OPENWALK_HUB_GIT_REF_ENV, "main");
    (repo, url_guard, ref_guard)
}

fn initialized_workspace() -> (TestDir, Workspace) {
    let sandbox = TestDir::new();
    // Tests construct the workspace from an isolated base dir instead of mutating the repo.
    let workspace = Workspace::from_base_dir(sandbox.path.clone());
    workspace.init().expect("workspace should initialize");
    (sandbox, workspace)
}

fn initialized_global_home() -> (TestDir, GlobalHome) {
    let sandbox = TestDir::new();
    let global_home = GlobalHome::from_root(sandbox.path.join("global-home"));
    global_home.init().expect("global home should initialize");
    (sandbox, global_home)
}

#[tokio::test(flavor = "multi_thread")]
async fn run_local_executes_scheme_script() {
    let _cwd_guard = CWD_LOCK
        .acquire()
        .await
        .expect("cwd lock should be acquired");
    let sandbox = TestDir::new();
    let previous_dir = env::current_dir().expect("cwd should be readable");
    env::set_current_dir(&sandbox.path).expect("should change cwd for the test");

    let workspace = Workspace::discover().expect("workspace should resolve");
    workspace.init().expect("workspace should initialize");
    let script_path = sandbox.path.join("double.scm");
    fs::write(&script_path, "(+ 19 23)").expect("script should be written");

    let result = run_local(ToolExecArgs {
        tool: script_path.display().to_string(),
        args: Vec::new(),
    })
    .await;

    env::set_current_dir(previous_dir).expect("cwd should be restored");
    result.expect("script should run");
}

#[tokio::test(flavor = "multi_thread")]
async fn run_local_resolves_workspace_tool_names() {
    let _cwd_guard = CWD_LOCK
        .acquire()
        .await
        .expect("cwd lock should be acquired");
    let sandbox = TestDir::new();
    let previous_dir = env::current_dir().expect("cwd should be readable");
    env::set_current_dir(&sandbox.path).expect("should change cwd for the test");

    let workspace = Workspace::discover().expect("workspace should resolve");
    workspace.init().expect("workspace should initialize");
    let tool_dir = workspace.tool_dir("smoke");
    fs::create_dir_all(&tool_dir).expect("tool dir should be created");
    fs::write(tool_dir.join("main.scm"), "(+ 20 22)").expect("script should be written");

    let result = run_local(ToolExecArgs {
        tool: "smoke".to_string(),
        args: Vec::new(),
    })
    .await;

    env::set_current_dir(previous_dir).expect("cwd should be restored");
    result.expect("workspace tool should run");
}

#[test]
fn resolve_script_target_returns_file_paths() {
    let sandbox = TestDir::new();
    let script_path = sandbox.path.join("demo.scm");
    fs::write(&script_path, "(+ 1 2)").expect("script should be written");

    let resolved = resolve_script_target(script_path.to_str().expect("valid utf8 path"))
        .expect("resolution should succeed");

    assert_eq!(resolved, Some(script_path));
}

#[test]
fn resolve_script_target_rejects_missing_scheme_files() {
    let err = resolve_script_target("./missing.scm").expect_err("missing script should fail");
    assert!(err.to_string().contains("was not found"));
}

#[test]
fn resolve_script_target_treats_namespaced_refs_as_tools() {
    let resolved = resolve_script_target("v2ex/hot").expect("resolution should succeed");
    assert_eq!(resolved, None);
}

#[test]
fn resolve_run_target_maps_tool_name_to_workspace_entry() {
    let sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(sandbox.path.clone());
    workspace.init().expect("workspace should initialize");

    let tool_dir = workspace.tool_dir("browser-smoke");
    fs::create_dir_all(&tool_dir).expect("tool dir should be created");
    let entry = tool_dir.join("main.scm");
    fs::write(&entry, "(+ 1 2)").expect("script should be written");

    let resolved =
        resolve_run_target(&workspace, "browser-smoke").expect("workspace tool should resolve");
    assert_eq!(resolved, entry);
}

#[test]
fn resolve_run_target_supports_namespaced_workspace_tools() {
    let sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(sandbox.path.clone());
    workspace.init().expect("workspace should initialize");

    let tool_dir = workspace.tool_dir("v2ex/hot");
    fs::create_dir_all(&tool_dir).expect("tool dir should be created");
    let entry = tool_dir.join("main.scm");
    fs::write(&entry, "(+ 1 2)").expect("script should be written");

    let resolved =
        resolve_run_target(&workspace, "v2ex/hot").expect("workspace tool should resolve");
    assert_eq!(resolved, entry);
}

#[test]
fn install_and_uninstall_package_updates_store() {
    let _env_guard = HUB_ENV_LOCK
        .lock()
        .expect("hub env lock should be acquired");
    let (_sandbox, workspace) = initialized_workspace();
    let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
        "browser-tools",
        r#"#| @meta
{
  "name": "browser-tools",
  "description": "Hub fixture workspace tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk exec browser-tools"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "workspace-ok")
"#,
    );

    install_package(&workspace, "browser-tools".to_string()).expect("install should succeed");
    let installed = workspace
        .load_tools()
        .expect("tools should load after install");
    assert_eq!(
        installed.packages,
        vec![InstalledPackage {
            name: "browser-tools".to_string(),
            version: None,
            path: None,
        }]
    );
    assert!(workspace.tool_entry_path("browser-tools").exists());

    uninstall_package(&workspace, "browser-tools".to_string()).expect("uninstall should succeed");
    let after_uninstall = workspace
        .load_tools()
        .expect("tools should load after uninstall");
    assert!(after_uninstall.packages.is_empty());
    assert!(!workspace.tool_dir("browser-tools").exists());
}

#[test]
fn install_workspace_tools_installs_declared_manifest_packages() {
    let _env_guard = HUB_ENV_LOCK
        .lock()
        .expect("hub env lock should be acquired");
    let sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(sandbox.path.clone());
    workspace
        .init_with_options(InitOptions {
            tools: vec!["browser-tools".to_string(), "v2ex-tools".to_string()],
            ..InitOptions::default()
        })
        .expect("workspace should initialize with declared tools");
    let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tools(&[
        (
            "browser-tools",
            r#"#| @meta
{
  "name": "browser-tools",
  "description": "Hub fixture workspace tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk install"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "workspace-ok")
"#,
        ),
        (
            "v2ex-tools",
            r#"#| @meta
{
  "name": "v2ex-tools",
  "description": "Second hub fixture tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk install"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "workspace-ok-2")
"#,
        ),
    ]);

    install_workspace_tools(&workspace, ProjectInstallArgs {})
        .expect("top-level install should install declared tools");

    assert!(workspace.tool_entry_path("browser-tools").exists());
    assert!(workspace.tool_entry_path("v2ex-tools").exists());
}

#[test]
fn install_workspace_tools_without_manifest_fails() {
    let sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(sandbox.path.clone());

    let error = install_workspace_tools(&workspace, ProjectInstallArgs {})
        .expect_err("install without manifest should fail");

    assert!(error.to_string().contains("openwalk init"));
}

#[test]
fn handle_tool_add_and_remove_updates_store() {
    let _env_guard = HUB_ENV_LOCK
        .lock()
        .expect("hub env lock should be acquired");
    let (_workspace_sandbox, workspace) = initialized_workspace();
    let (_global_sandbox, global_home) = initialized_global_home();
    let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
        "browser-tools",
        r#"#| @meta
{
  "name": "browser-tools",
  "description": "Hub fixture workspace tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk tool add browser-tools"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "workspace-ok")
"#,
    );

    handle_tool_command(
        &workspace,
        &global_home,
        ToolCommand::Add {
            package: "browser-tools".to_string(),
        },
    )
    .expect("tool add should succeed");

    let installed = workspace
        .load_tools()
        .expect("tools should load after tool add");
    assert_eq!(
        installed.packages,
        vec![InstalledPackage {
            name: "browser-tools".to_string(),
            version: None,
            path: None,
        }]
    );
    assert!(workspace.tool_entry_path("browser-tools").exists());

    handle_tool_command(
        &workspace,
        &global_home,
        ToolCommand::Remove {
            package: "browser-tools".to_string(),
        },
    )
    .expect("tool remove should succeed");

    let after_remove = workspace
        .load_tools()
        .expect("tools should load after tool remove");
    assert!(after_remove.packages.is_empty());
    assert!(!workspace.tool_dir("browser-tools").exists());
}

#[test]
fn handle_tool_install_and_uninstall_updates_global_store_and_shim() {
    let _env_guard = HUB_ENV_LOCK
        .lock()
        .expect("hub env lock should be acquired");
    let (_workspace_sandbox, workspace) = initialized_workspace();
    let (_global_sandbox, global_home) = initialized_global_home();
    let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
        "browser-tools",
        r#"#| @meta
{
  "name": "browser-tools",
  "description": "Hub fixture global tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk tool install browser-tools"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "global-ok")
"#,
    );

    handle_tool_command(
        &workspace,
        &global_home,
        ToolCommand::Install {
            package: "browser-tools".to_string(),
        },
    )
    .expect("tool install should succeed");

    let installed = global_home
        .load_tools()
        .expect("global tools should load after tool install");
    assert_eq!(
        installed.packages,
        vec![InstalledPackage {
            name: "browser-tools".to_string(),
            version: None,
            path: None,
        }]
    );
    assert!(global_home.tool_entry_path("browser-tools").exists());
    assert!(global_home.shim_path("browser-tools").exists());

    handle_tool_command(
        &workspace,
        &global_home,
        ToolCommand::Uninstall {
            package: "browser-tools".to_string(),
        },
    )
    .expect("tool uninstall should succeed");

    let after_uninstall = global_home
        .load_tools()
        .expect("global tools should load after tool uninstall");
    assert!(after_uninstall.packages.is_empty());
    assert!(!global_home.shim_path("browser-tools").exists());
    assert!(!global_home.tool_dir("browser-tools").exists());
}

#[test]
fn load_tool_info_reads_workspace_script_metadata() {
    let (_sandbox, workspace) = initialized_workspace();
    let (_global_sandbox, global_home) = initialized_global_home();
    let tool_dir = workspace.tool_dir("bing-search");
    fs::create_dir_all(&tool_dir).expect("tool dir should be created");
    fs::write(
        tool_dir.join("main.scm"),
        r#"#| @meta
{
  "name": "bing-search",
  "description": "Bing 搜索",
  "args": [],
  "returns": {
    "type": "object",
    "description": "{ results[] }"
  },
  "examples": ["openwalk run bing-search -- \"Claude Code\" 10"],
  "domains": ["www.bing.com"],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["search"]
}
|#
(define (main args) "ok")
"#,
    )
    .expect("script should be written");

    let info =
        load_tool_info(&workspace, &global_home, "bing-search").expect("tool info should load");

    assert_eq!(info.name, "bing-search");
    assert_eq!(info.source, "workspace-tool");
    assert_eq!(info.metadata.description, "Bing 搜索");
}

#[test]
fn load_tool_info_reads_global_script_metadata() {
    let _env_guard = HUB_ENV_LOCK
        .lock()
        .expect("hub env lock should be acquired");
    let workspace_sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
    let (_global_sandbox, global_home) = initialized_global_home();
    let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
        "global-bing-search",
        r#"#| @meta
{
  "name": "global-bing-search",
  "description": "全局 Bing 搜索",
  "args": [],
  "returns": {
    "type": "object",
    "description": "{ results[] }"
  },
  "examples": ["openwalk exec global-bing-search"],
  "domains": ["www.bing.com"],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["search"]
}
|#
(define (main args) "ok")
"#,
    );

    install_global_package(&global_home, "global-bing-search".to_string())
        .expect("global install should succeed");

    let info = load_tool_info(&workspace, &global_home, "global-bing-search")
        .expect("global tool info should load");

    assert_eq!(info.name, "global-bing-search");
    assert_eq!(info.source, "global-tool");
    assert_eq!(info.metadata.description, "全局 Bing 搜索");
}

#[test]
fn workspace_tool_entries_include_metadata_description() {
    let (_sandbox, workspace) = initialized_workspace();
    let tool_dir = workspace.tool_dir("bing-search");
    fs::create_dir_all(&tool_dir).expect("tool dir should be created");
    fs::write(
        tool_dir.join("main.scm"),
        r#"#| @meta
{
  "name": "bing-search",
  "description": "Bing 搜索并返回结构化结果",
  "args": [],
  "returns": {
    "type": "object",
    "description": "{ results[] }"
  },
  "examples": ["openwalk run bing-search -- \"Claude Code\" 10"],
  "domains": ["www.bing.com"],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["search"]
}
|#
(define (main args) "ok")
"#,
    )
    .expect("script should be written");

    let entries = workspace_tool_entries(&workspace).expect("workspace tools should load");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "bing-search");
    assert_eq!(entries[0].usage, "bing-search");
    assert_eq!(entries[0].description, "Bing 搜索并返回结构化结果");
    assert_eq!(entries[0].source, "workspace");
}

#[test]
fn tool_usage_from_metadata_renders_required_and_optional_args() {
    let metadata =
        builtin_tools::builtin_tool_metadata("tab-new").expect("tab-new metadata should exist");
    assert_eq!(
        tool_usage_from_metadata("tab-new", &metadata.args),
        "tab-new [url]"
    );

    let metadata = builtin_tools::builtin_tool_metadata("browser-open")
        .expect("browser-open metadata should exist");
    assert_eq!(
        tool_usage_from_metadata("browser-open", &metadata.args),
        "browser-open <url>"
    );
}

#[test]
fn tool_help_request_requires_help_as_the_only_tool_arg() {
    assert!(exec::is_tool_help_request(&["--help".to_string()]));
    assert!(exec::is_tool_help_request(&["-h".to_string()]));
    assert!(!exec::is_tool_help_request(&[
        "--help".to_string(),
        "extra".to_string()
    ]));
    assert!(!exec::is_tool_help_request(&["--headed".to_string()]));
}

#[test]
fn render_tool_list_lines_aligns_usage_column() {
    let lines = render_tool_list_lines(&[
        ToolListEntry {
            name: "browser-open".to_string(),
            usage: "browser-open <url>".to_string(),
            description: "打开浏览器并导航".to_string(),
            source: "builtin".to_string(),
        },
        ToolListEntry {
            name: "tab-list".to_string(),
            usage: "tab-list".to_string(),
            description: "列出所有标签页".to_string(),
            source: "builtin".to_string(),
        },
    ]);

    assert_eq!(lines[0], "browser-open <url>  打开浏览器并导航");
    assert_eq!(lines[1], "tab-list            列出所有标签页");
}

#[test]
fn load_tool_info_reads_builtin_host_function_metadata() {
    let workspace_sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
    let (_global_sandbox, global_home) = initialized_global_home();

    let info =
        load_tool_info(&workspace, &global_home, "browser-open").expect("builtin info should load");

    assert_eq!(info.name, "browser-open");
    assert_eq!(info.source, "host-function");
    assert_eq!(info.script, None);
    assert_eq!(info.metadata.description, "打开浏览器并导航到指定 URL。");
}

#[test]
fn build_tool_info_view_flattens_builtin_metadata() {
    let info = build_builtin_tool_info("browser-open").expect("builtin info should load");
    let view = build_tool_info_view(&info, "browser-open");

    assert_eq!(view.name, "browser-open");
    assert_eq!(view.usage, "browser-open <url>");
    assert_eq!(view.description, "打开浏览器并导航到指定 URL");
    assert_eq!(view.source, "builtin");
    assert!(view.script.is_none());
    assert_eq!(view.args.len(), 1);
    assert_eq!(view.options.len(), 4);
    assert_eq!(view.options[0].name, "-s, --session <name>");
    assert_eq!(view.options[1].name, "--headed");
    assert_eq!(view.options[2].name, "--new-tab");
    assert_eq!(view.options[3].name, "--profile <path>");
}

#[test]
fn build_tool_info_view_uses_script_target_for_direct_scripts() {
    let script_path = PathBuf::from("/tmp/demo.scm");
    let info = ToolInfoEntry {
        name: "demo".to_string(),
        source: "script-path".to_string(),
        script: Some(script_path.display().to_string()),
        metadata: ToolMetadata {
            name: "demo".to_string(),
            description: "演示脚本".to_string(),
            args: vec![ToolArgument {
                name: "name".to_string(),
                arg_type: "string".to_string(),
                required: false,
                default: None,
                description: "名字".to_string(),
            }],
            returns: ToolReturn {
                return_type: "string".to_string(),
                description: "ok".to_string(),
            },
            examples: vec!["openwalk run ./demo.scm -- Bloom".to_string()],
            domains: Vec::new(),
            read_only: true,
            requires_login: false,
            tags: vec!["demo".to_string()],
        },
    };

    let view = build_tool_info_view(&info, "./demo.scm");

    assert_eq!(view.source, "script");
    assert_eq!(view.usage, "./demo.scm [name]");
    assert!(view.options.is_empty());
    assert_eq!(view.script.as_deref(), Some("/tmp/demo.scm"));
}

#[test]
fn load_tool_info_reads_direct_script_metadata() {
    let workspace_sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
    let (_global_sandbox, global_home) = initialized_global_home();
    let script_path = workspace_sandbox.path.join("demo.scm");
    fs::write(
        &script_path,
        r#"#| @meta
{
  "name": "demo",
  "description": "演示脚本",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk tool info ./demo.scm"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["demo"]
}
|#
(define (main args) "ok")
"#,
    )
    .expect("script should be written");

    let info = load_tool_info(
        &workspace,
        &global_home,
        script_path.to_str().expect("valid utf8 path"),
    )
    .expect("tool info should load");

    assert_eq!(info.name, "demo");
    assert_eq!(info.source, "script-path");
}

#[test]
fn load_tool_info_rejects_missing_metadata_header() {
    let workspace_sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
    let (_global_sandbox, global_home) = initialized_global_home();
    let script_path = workspace_sandbox.path.join("demo.scm");
    fs::write(&script_path, "(define (main args) \"ok\")").expect("script should be written");

    let err = load_tool_info(
        &workspace,
        &global_home,
        script_path.to_str().expect("valid utf8 path"),
    )
    .expect_err("missing metadata should fail");

    assert!(err
        .to_string()
        .contains("missing a `#| @meta ... |#` header"));
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_tool_auto_installs_unknown_tools_from_hub() {
    let _env_guard = HUB_ENV_LOCK
        .lock()
        .expect("hub env lock should be acquired");
    let workspace_sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
    let (_global_sandbox, global_home) = initialized_global_home();
    let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
        "remote.browser.open",
        r#"#| @meta
{
  "name": "remote.browser.open",
  "description": "Remote fixture tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk exec remote.browser.open"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "remote-ok")
"#,
    );

    exec_tool(
        &workspace,
        &global_home,
        ToolExecArgs {
            tool: "remote.browser.open".to_string(),
            args: vec!["https://example.com".to_string()],
        },
    )
    .await
    .expect("exec should install and run the hub tool");

    let installed = workspace
        .load_tools()
        .expect("tools should load after remote exec install");
    assert!(package_exists(&installed, "remote.browser.open"));
    assert!(workspace.tool_entry_path("remote.browser.open").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_tool_auto_installs_namespaced_tools_from_hub() {
    let _env_guard = HUB_ENV_LOCK
        .lock()
        .expect("hub env lock should be acquired");
    let workspace_sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
    let (_global_sandbox, global_home) = initialized_global_home();
    let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
        "v2ex/hot",
        r#"#| @meta
{
  "name": "v2ex/hot",
  "description": "Namespaced fixture tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk exec v2ex/hot"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "remote-ok")
"#,
    );

    exec_tool(
        &workspace,
        &global_home,
        ToolExecArgs {
            tool: "v2ex/hot".to_string(),
            args: Vec::new(),
        },
    )
    .await
    .expect("exec should install and run the namespaced hub tool");

    let installed = workspace
        .load_tools()
        .expect("tools should load after remote exec install");
    assert!(package_exists(&installed, "v2ex/hot"));
    assert!(workspace.tool_entry_path("v2ex/hot").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_tool_executes_builtin_host_function() {
    let workspace_sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
    let (_global_sandbox, global_home) = initialized_global_home();

    exec_tool(
        &workspace,
        &global_home,
        ToolExecArgs {
            tool: "time-sleep".to_string(),
            args: vec!["0".to_string()],
        },
    )
    .await
    .expect("exec should run builtin host functions");
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_tool_allows_global_packages_without_workspace_init() {
    let _env_guard = HUB_ENV_LOCK
        .lock()
        .expect("hub env lock should be acquired");
    let workspace_sandbox = TestDir::new();
    let workspace = Workspace::from_base_dir(workspace_sandbox.path.clone());
    let (_global_sandbox, global_home) = initialized_global_home();
    let (_repo, _hub_url_guard, _hub_ref_guard) = install_test_hub_tool(
        "browser-tools",
        r#"#| @meta
{
  "name": "browser-tools",
  "description": "Global fixture tool",
  "args": [],
  "returns": {
    "type": "string",
    "description": "ok"
  },
  "examples": ["openwalk exec browser-tools"],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["fixture"]
}
|#
(define (main args) "global-ok")
"#,
    );

    install_global_package(&global_home, "browser-tools".to_string())
        .expect("global install should succeed");

    exec_tool(
        &workspace,
        &global_home,
        ToolExecArgs {
            tool: "browser-tools".to_string(),
            args: vec!["https://example.com".to_string()],
        },
    )
    .await
    .expect("exec should allow globally installed packages without a workspace");

    assert!(!workspace.is_initialized());
}

#[tokio::test(flavor = "multi_thread")]
async fn run_local_rejects_builtin_host_function_names() {
    let _cwd_guard = CWD_LOCK
        .acquire()
        .await
        .expect("cwd lock should be acquired");
    let sandbox = TestDir::new();
    let previous_dir = env::current_dir().expect("cwd should be readable");
    env::set_current_dir(&sandbox.path).expect("should change cwd for the test");

    let result = run_local(ToolExecArgs {
        tool: "browser-open".to_string(),
        args: vec!["https://example.com".to_string()],
    })
    .await;

    env::set_current_dir(previous_dir).expect("cwd should be restored");

    let err = result.expect_err("run should reject builtin host functions");
    assert!(err
        .to_string()
        .contains("Use `openwalk exec browser-open` instead"));
}
