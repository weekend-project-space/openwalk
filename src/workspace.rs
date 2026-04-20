use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const WORKSPACE_DIR: &str = ".openwalk";
const OPENWALK_HOME_ENV: &str = "OPENWALK_HOME";
const MANIFEST_FILE: &str = "openwalk.json";
const GLOBAL_REPO_DIR: &str = "repo";
const LEGACY_CONFIG_FILE: &str = "config.json";
const TOOLS_DIR: &str = "tools";
const BIN_DIR: &str = "bin";
const RUN_DIR: &str = "run";
const BROWSER_RUN_DIR: &str = "browser";
const BROWSER_PROFILES_DIR: &str = "browser-profile";
const DEFAULT_BROWSER_PROFILE: &str = "default";
const TOOL_ENTRY_FILE: &str = "main.scm";

#[derive(Debug, Clone)]
// Points at the project root and runtime `.openwalk` directory for a chosen base path.
pub struct Workspace {
    base_dir: PathBuf,
    root: PathBuf,
}

#[derive(Debug, Clone)]
// Points at the on-disk global openwalk home, defaulting to ~/.openwalk.
pub struct GlobalHome {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub package: PackageManifest,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolDependency>,
}

impl WorkspaceManifest {
    fn for_workspace(base_dir: &Path) -> Self {
        Self {
            package: PackageManifest::for_workspace(base_dir),
            tools: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub private: bool,
}

impl PackageManifest {
    fn for_workspace(base_dir: &Path) -> Self {
        Self {
            name: default_package_name(base_dir),
            version: "0.1.0".to_string(),
            description: None,
            private: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolDependency {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Installed packages are stored as a tiny Phase 1 record and expanded later as metadata grows.
pub struct InstalledPackage {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
// Flat store for locally installed packages.
pub struct ToolStore {
    pub packages: Vec<InstalledPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct GlobalManifest {
    #[serde(default)]
    tools: BTreeMap<String, ToolDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTool {
    pub name: String,
    pub entry_path: PathBuf,
}

#[derive(Debug)]
// Reports which pieces were created so `openwalk init` can explain whether it changed anything.
pub struct InitSummary {
    pub created_root: bool,
    pub created_manifest: bool,
    pub created_tool_dir: bool,
    pub overwritten_manifest: bool,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
// Options the CLI forwards to `Workspace::init_with_options`.
pub struct InitOptions {
    pub name: Option<String>,
    pub tools: Vec<String>,
    pub force: bool,
}

impl Workspace {
    pub fn discover() -> Result<Self> {
        let cwd = env::current_dir().context("failed to determine current directory")?;
        Ok(Self::from_base_dir(cwd))
    }

    pub(crate) fn from_base_dir(base_dir: PathBuf) -> Self {
        let root = base_dir.join(WORKSPACE_DIR);
        Self { base_dir, root }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.base_dir.join(MANIFEST_FILE)
    }

    pub fn is_initialized(&self) -> bool {
        self.root.exists()
            && self.tools_dir().exists()
            && (self.manifest_path().exists() || self.legacy_is_initialized())
    }

    pub fn tools_dir(&self) -> PathBuf {
        self.root.join(TOOLS_DIR)
    }

    pub fn tool_dir(&self, tool_name: &str) -> PathBuf {
        self.tools_dir().join(tool_name)
    }

    pub fn tool_entry_path(&self, tool_name: &str) -> PathBuf {
        self.tool_dir(tool_name).join(TOOL_ENTRY_FILE)
    }

    pub fn local_tools(&self) -> Result<Vec<LocalTool>> {
        let tools_dir = self.tools_dir();
        if !tools_dir.exists() {
            return Ok(Vec::new());
        }

        let mut tools = Vec::new();
        for entry in fs::read_dir(&tools_dir)
            .with_context(|| format!("failed to read {}", tools_dir.display()))?
        {
            let entry = entry.with_context(|| {
                format!(
                    "failed to inspect directory entry under {}",
                    tools_dir.display()
                )
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let entry_path = path.join(TOOL_ENTRY_FILE);
            if !entry_path.is_file() {
                continue;
            }

            tools.push(LocalTool {
                name: entry.file_name().to_string_lossy().into_owned(),
                entry_path,
            });
        }

        tools.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(tools)
    }

    #[cfg(test)]
    pub fn init(&self) -> Result<InitSummary> {
        self.init_with_options(InitOptions::default())
    }

    pub fn init_with_options(&self, options: InitOptions) -> Result<InitSummary> {
        let created_root = if self.root.exists() {
            false
        } else {
            fs::create_dir_all(&self.root).with_context(|| {
                format!("failed to create workspace at {}", self.root.display())
            })?;
            true
        };

        let manifest_path = self.manifest_path();
        let has_manifest_options = options.name.is_some() || !options.tools.is_empty();

        let (created_manifest, overwritten_manifest, backup_path) = if !manifest_path.exists() {
            let manifest = self.build_fresh_manifest(&options)?;
            self.write_json(&manifest_path, &manifest)?;
            (true, false, None)
        } else if options.force {
            let backup = self.base_dir.join(format!("{MANIFEST_FILE}.bak"));
            fs::rename(&manifest_path, &backup).with_context(|| {
                format!(
                    "failed to back up {} to {}",
                    manifest_path.display(),
                    backup.display()
                )
            })?;
            let manifest = self.build_fresh_manifest(&options)?;
            self.write_json(&manifest_path, &manifest)?;
            (false, true, Some(backup))
        } else if has_manifest_options {
            bail!(
                "workspace already initialized at {}. Pass `--force` to reset or edit `{}` directly.",
                self.root.display(),
                manifest_path.display()
            );
        } else {
            (false, false, None)
        };

        let tools_dir = self.tools_dir();
        let created_tool_dir = if tools_dir.exists() {
            false
        } else {
            fs::create_dir_all(&tools_dir).with_context(|| {
                format!(
                    "failed to create tools directory at {}",
                    tools_dir.display()
                )
            })?;
            true
        };

        Ok(InitSummary {
            created_root,
            created_manifest,
            created_tool_dir,
            overwritten_manifest,
            backup_path,
        })
    }

    fn build_fresh_manifest(&self, options: &InitOptions) -> Result<WorkspaceManifest> {
        let mut manifest = if self.legacy_is_initialized() {
            self.import_legacy_manifest()?
        } else {
            WorkspaceManifest::for_workspace(self.base_dir())
        };

        if let Some(name) = options.name.as_deref() {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                bail!("`--name` must not be empty");
            }
            manifest.package.name = trimmed.to_string();
        }

        for tool in options
            .tools
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            manifest.tools.entry(tool.to_string()).or_default();
        }

        Ok(manifest)
    }

    pub fn ensure_initialized(&self) -> Result<()> {
        if !self.root.exists() {
            bail!(
                "workspace runtime directory not initialized at {}. Run `openwalk init` first.",
                self.root.display()
            );
        }

        if !self.tools_dir().exists() {
            bail!(
                "workspace tools directory is incomplete at {}. Run `openwalk init` again.",
                self.tools_dir().display()
            );
        }

        if !self.manifest_path().exists() && !self.legacy_is_initialized() {
            bail!(
                "workspace manifest not found at {}. Run `openwalk init` first.",
                self.manifest_path().display()
            );
        }

        Ok(())
    }

    pub fn load_manifest(&self) -> Result<WorkspaceManifest> {
        self.ensure_initialized()?;

        if self.manifest_path().exists() {
            let manifest_path = self.manifest_path();
            let data = fs::read_to_string(&manifest_path)
                .with_context(|| format!("failed to read {}", manifest_path.display()))?;
            let manifest = serde_json::from_str(&data)
                .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
            return Ok(manifest);
        }

        self.import_legacy_manifest()
    }

    pub fn load_manifest_or_default(&self) -> Result<WorkspaceManifest> {
        if self.is_initialized() {
            self.load_manifest()
        } else {
            Ok(WorkspaceManifest::for_workspace(self.base_dir()))
        }
    }

    pub fn save_manifest(&self, manifest: &WorkspaceManifest) -> Result<()> {
        self.ensure_initialized()?;
        self.write_json(self.manifest_path(), manifest)
    }

    pub fn load_tools(&self) -> Result<ToolStore> {
        let manifest = self.load_manifest()?;
        Ok(manifest_to_tool_store(manifest))
    }

    pub fn load_tools_or_default(&self) -> Result<ToolStore> {
        if self.is_initialized() {
            self.load_tools()
        } else {
            Ok(ToolStore::default())
        }
    }

    pub fn save_tools(&self, store: &ToolStore) -> Result<()> {
        let mut manifest = self.load_manifest_or_default()?;
        manifest.tools = tool_store_to_manifest_map(store);
        self.save_manifest(&manifest)
    }

    fn legacy_is_initialized(&self) -> bool {
        self.legacy_config_path().exists()
    }

    fn legacy_config_path(&self) -> PathBuf {
        self.root.join(LEGACY_CONFIG_FILE)
    }

    fn import_legacy_manifest(&self) -> Result<WorkspaceManifest> {
        let package = if self.legacy_config_path().exists() {
            let data = fs::read_to_string(self.legacy_config_path()).with_context(|| {
                format!(
                    "failed to read legacy workspace config {}",
                    self.legacy_config_path().display()
                )
            })?;
            let config: LegacyWorkspaceConfig = serde_json::from_str(&data).with_context(|| {
                format!(
                    "failed to parse legacy workspace config {}",
                    self.legacy_config_path().display()
                )
            })?;
            PackageManifest {
                version: config.version,
                ..PackageManifest::for_workspace(self.base_dir())
            }
        } else {
            PackageManifest::for_workspace(self.base_dir())
        };

        Ok(WorkspaceManifest {
            package,
            tools: BTreeMap::new(),
        })
    }

    fn write_json<P, T>(&self, path: P, value: &T) -> Result<()>
    where
        P: AsRef<Path>,
        T: Serialize,
    {
        let bytes = serde_json::to_vec_pretty(value).context("failed to serialize json")?;
        fs::write(path.as_ref(), bytes)
            .with_context(|| format!("failed to write {}", path.as_ref().display()))?;
        Ok(())
    }
}

impl GlobalHome {
    pub fn discover() -> Result<Self> {
        if let Some(root) = env::var_os(OPENWALK_HOME_ENV) {
            return Ok(Self {
                root: PathBuf::from(root),
            });
        }

        let home_dir = user_home_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "failed to determine openwalk home directory. Set `{OPENWALK_HOME_ENV}` first."
            )
        })?;

        Ok(Self {
            root: home_dir.join(WORKSPACE_DIR),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.root.join(BIN_DIR)
    }

    pub fn tools_dir(&self) -> PathBuf {
        self.root.join(GLOBAL_REPO_DIR).join(TOOLS_DIR)
    }

    pub fn tool_dir(&self, tool_name: &str) -> PathBuf {
        self.tools_dir().join(tool_name)
    }

    pub fn tool_entry_path(&self, tool_name: &str) -> PathBuf {
        self.tool_dir(tool_name).join(TOOL_ENTRY_FILE)
    }

    pub fn browser_profiles_dir(&self) -> PathBuf {
        self.root.join(BROWSER_PROFILES_DIR)
    }

    pub fn run_dir(&self) -> PathBuf {
        self.root.join(RUN_DIR)
    }

    pub fn browser_sessions_dir(&self) -> PathBuf {
        self.run_dir().join(BROWSER_RUN_DIR)
    }

    pub fn browser_session_dir(&self, session_name: &str) -> PathBuf {
        self.browser_sessions_dir().join(session_name)
    }

    pub fn browser_profile_dir(&self, profile_name: &str) -> PathBuf {
        self.browser_profiles_dir().join(profile_name)
    }

    pub fn default_browser_profile_dir(&self) -> PathBuf {
        self.browser_profile_dir(DEFAULT_BROWSER_PROFILE)
    }

    pub fn init(&self) -> Result<()> {
        if !self.root.exists() {
            fs::create_dir_all(&self.root).with_context(|| {
                format!("failed to create global home at {}", self.root.display())
            })?;
        }

        let manifest_path = self.manifest_path();
        if !manifest_path.exists() {
            self.write_json(&manifest_path, &GlobalManifest::default())?;
        }

        let bin_dir = self.bin_dir();
        if !bin_dir.exists() {
            fs::create_dir_all(&bin_dir).with_context(|| {
                format!(
                    "failed to create global bin directory at {}",
                    bin_dir.display()
                )
            })?;
        }

        let tools_dir = self.tools_dir();
        if !tools_dir.exists() {
            fs::create_dir_all(&tools_dir).with_context(|| {
                format!(
                    "failed to create global tools directory at {}",
                    tools_dir.display()
                )
            })?;
        }

        let browser_sessions_dir = self.browser_sessions_dir();
        if !browser_sessions_dir.exists() {
            fs::create_dir_all(&browser_sessions_dir).with_context(|| {
                format!(
                    "failed to create browser sessions directory at {}",
                    browser_sessions_dir.display()
                )
            })?;
        }

        let profile_dir = self.default_browser_profile_dir();
        if !profile_dir.exists() {
            fs::create_dir_all(&profile_dir).with_context(|| {
                format!(
                    "failed to create default browser profile directory at {}",
                    profile_dir.display()
                )
            })?;
        }

        Ok(())
    }

    pub fn load_tools(&self) -> Result<ToolStore> {
        let manifest_path = self.manifest_path();
        if !manifest_path.exists() {
            return Ok(ToolStore::default());
        }

        let data = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest: GlobalManifest = serde_json::from_str(&data)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        Ok(global_manifest_to_tool_store(manifest))
    }

    pub fn save_tools(&self, store: &ToolStore) -> Result<()> {
        self.init()?;
        self.write_json(self.manifest_path(), &tool_store_to_global_manifest(store))
    }

    pub fn shim_path(&self, package: &str) -> PathBuf {
        let name = package.replace(std::path::MAIN_SEPARATOR, "-");

        #[cfg(windows)]
        {
            return self.bin_dir().join(format!("{name}.cmd"));
        }

        #[cfg(not(windows))]
        {
            self.bin_dir().join(name)
        }
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join(MANIFEST_FILE)
    }

    fn write_json<P, T>(&self, path: P, value: &T) -> Result<()>
    where
        P: AsRef<Path>,
        T: Serialize,
    {
        let bytes = serde_json::to_vec_pretty(value).context("failed to serialize json")?;
        fs::write(path.as_ref(), bytes)
            .with_context(|| format!("failed to write {}", path.as_ref().display()))?;
        Ok(())
    }
}

fn manifest_to_tool_store(manifest: WorkspaceManifest) -> ToolStore {
    let mut packages = manifest
        .tools
        .into_iter()
        .map(|(name, dependency)| InstalledPackage {
            name,
            version: dependency.version,
            path: dependency.path,
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    ToolStore { packages }
}

fn tool_store_to_manifest_map(store: &ToolStore) -> BTreeMap<String, ToolDependency> {
    let mut tools = BTreeMap::new();
    for package in &store.packages {
        tools.insert(
            package.name.clone(),
            ToolDependency {
                version: package.version.clone(),
                path: package.path.clone(),
            },
        );
    }
    tools
}

fn global_manifest_to_tool_store(manifest: GlobalManifest) -> ToolStore {
    let mut packages = manifest
        .tools
        .into_iter()
        .map(|(name, dependency)| InstalledPackage {
            name,
            version: dependency.version,
            path: dependency.path,
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    ToolStore { packages }
}

fn tool_store_to_global_manifest(store: &ToolStore) -> GlobalManifest {
    GlobalManifest {
        tools: tool_store_to_manifest_map(store),
    }
}

fn default_package_name(base_dir: &Path) -> String {
    base_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.trim().replace(' ', "-"))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "openwalk-project".to_string())
}

fn user_home_dir() -> Option<PathBuf> {
    if let Some(path) = env::var_os("HOME") {
        return Some(PathBuf::from(path));
    }

    if let Some(path) = env::var_os("USERPROFILE") {
        return Some(PathBuf::from(path));
    }

    match (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
        (Some(drive), Some(path)) => {
            let mut joined = PathBuf::from(drive);
            joined.push(path);
            Some(joined)
        }
        _ => None,
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LegacyWorkspaceConfig {
    version: String,
}

#[cfg(test)]
mod tests {
    use std::{
        process,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let nonce = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "openwalk-workspace-test-{}-{timestamp}-{nonce}",
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

    #[test]
    fn init_creates_workspace_manifest_and_runtime_dir() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());

        let first = workspace.init().expect("workspace should initialize");
        assert!(first.created_root);
        assert!(first.created_manifest);
        assert!(first.created_tool_dir);
        assert!(workspace.root.exists());
        assert!(workspace.manifest_path().exists());
        assert!(workspace.tools_dir().exists());

        let manifest = workspace.load_manifest().expect("manifest should load");
        assert!(manifest
            .package
            .name
            .starts_with("openwalk-workspace-test-"));
        assert_eq!(manifest.package.version, "0.1.0");
        assert!(manifest.package.private);
        assert!(manifest.tools.is_empty());

        let second = workspace.init().expect("workspace re-init should succeed");
        assert!(!second.created_root);
        assert!(!second.created_manifest);
        assert!(!second.created_tool_dir);
    }

    #[test]
    fn save_and_load_tools_round_trip() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        workspace.init().expect("workspace should initialize");

        let store = ToolStore {
            packages: vec![InstalledPackage {
                name: "browser-tools".to_string(),
                version: Some("^0.1.0".to_string()),
                path: None,
            }],
        };

        workspace
            .save_tools(&store)
            .expect("tool store should be saved");

        let loaded = workspace.load_tools().expect("tool store should load");
        assert_eq!(loaded, store);

        let manifest = workspace.load_manifest().expect("manifest should load");
        assert_eq!(
            manifest.tools.get("browser-tools"),
            Some(&ToolDependency {
                version: Some("^0.1.0".to_string()),
                path: None,
            })
        );
    }

    #[test]
    fn load_manifest_migrates_legacy_workspace_config() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        fs::create_dir_all(&workspace.root).expect("runtime dir should exist");
        fs::create_dir_all(workspace.tools_dir()).expect("tools dir should exist");
        fs::write(workspace.legacy_config_path(), r#"{"version":"0.9.0"}"#)
            .expect("legacy config should be written");

        let manifest = workspace.load_manifest().expect("manifest should import");
        assert_eq!(manifest.package.version, "0.9.0");
        assert!(manifest.tools.is_empty());
    }

    #[test]
    fn init_with_options_applies_name_and_tools_on_fresh_workspace() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());

        let summary = workspace
            .init_with_options(InitOptions {
                name: Some("my-walk".to_string()),
                tools: vec!["v2ex-hot".to_string(), "bing-search".to_string()],
                force: false,
            })
            .expect("workspace should initialize with options");

        assert!(summary.created_manifest);
        assert!(!summary.overwritten_manifest);
        assert!(summary.backup_path.is_none());

        let manifest = workspace.load_manifest().expect("manifest should load");
        assert_eq!(manifest.package.name, "my-walk");
        assert_eq!(manifest.tools.len(), 2);
        assert!(manifest.tools.contains_key("v2ex-hot"));
        assert!(manifest.tools.contains_key("bing-search"));
    }

    #[test]
    fn init_with_options_rejects_rename_on_existing_without_force() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        workspace.init().expect("initial init should succeed");

        let error = workspace
            .init_with_options(InitOptions {
                name: Some("renamed".to_string()),
                ..InitOptions::default()
            })
            .expect_err("rename without --force should be rejected");

        assert!(error.to_string().contains("--force"));
    }

    #[test]
    fn init_with_options_force_backs_up_and_rewrites_manifest() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        workspace.init().expect("initial init should succeed");

        let original = fs::read_to_string(workspace.manifest_path())
            .expect("original manifest should be readable");

        let summary = workspace
            .init_with_options(InitOptions {
                name: Some("renamed".to_string()),
                tools: vec!["bing-search".to_string()],
                force: true,
            })
            .expect("force reinit should succeed");

        assert!(summary.overwritten_manifest);
        assert!(!summary.created_manifest);
        let backup = summary.backup_path.expect("backup path should be reported");
        assert_eq!(
            fs::read_to_string(&backup).expect("backup file should be readable"),
            original
        );

        let manifest = workspace.load_manifest().expect("manifest should load");
        assert_eq!(manifest.package.name, "renamed");
        assert!(manifest.tools.contains_key("bing-search"));
    }

    #[test]
    fn init_with_options_force_heals_corrupted_manifest() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        workspace.init().expect("initial init should succeed");

        fs::write(workspace.manifest_path(), "not valid json")
            .expect("manifest should be overwritten with garbage");

        workspace
            .init_with_options(InitOptions {
                force: true,
                ..InitOptions::default()
            })
            .expect("force reinit should heal corrupted manifest");

        workspace
            .load_manifest()
            .expect("manifest should load after force reinit");
    }

    #[test]
    fn init_with_options_rejects_empty_name() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());

        let error = workspace
            .init_with_options(InitOptions {
                name: Some("   ".to_string()),
                ..InitOptions::default()
            })
            .expect_err("blank name should be rejected");
        assert!(error.to_string().contains("--name"));
    }

    #[test]
    fn local_tools_discovers_runtime_scripts() {
        let sandbox = TestDir::new();
        let workspace = Workspace::from_base_dir(sandbox.path.clone());
        workspace.init().expect("workspace should initialize");

        let tool_dir = workspace.tool_dir("bing-search");
        fs::create_dir_all(&tool_dir).expect("tool dir should be created");
        fs::write(tool_dir.join("main.scm"), "(define (main args) \"ok\")")
            .expect("script should be written");

        let tools = workspace.local_tools().expect("local tools should load");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "bing-search");
    }

    #[test]
    fn global_home_init_creates_store_and_bin_dir() {
        let sandbox = TestDir::new();
        let global_home = GlobalHome::from_root(sandbox.path.join("global-home"));

        global_home.init().expect("global home should initialize");

        assert!(global_home.root.exists());
        assert!(global_home.manifest_path().exists());
        assert!(global_home.tools_dir().exists());
        assert!(global_home.bin_dir().exists());
        assert!(global_home.default_browser_profile_dir().exists());
        assert_eq!(
            global_home
                .load_tools()
                .expect("global tool store should load"),
            ToolStore::default()
        );
    }

    #[test]
    fn global_home_default_browser_profile_dir_is_under_openwalk_home() {
        let sandbox = TestDir::new();
        let global_home = GlobalHome::from_root(sandbox.path.join("global-home"));

        assert_eq!(
            global_home.default_browser_profile_dir(),
            sandbox
                .path
                .join("global-home")
                .join("browser-profile")
                .join("default")
        );
    }

    #[test]
    fn global_home_save_and_load_tools_round_trip() {
        let sandbox = TestDir::new();
        let global_home = GlobalHome::from_root(sandbox.path.join("global-home"));
        global_home.init().expect("global home should initialize");

        let store = ToolStore {
            packages: vec![InstalledPackage {
                name: "browser-tools".to_string(),
                version: None,
                path: None,
            }],
        };

        global_home
            .save_tools(&store)
            .expect("global tool store should be saved");

        let loaded = global_home
            .load_tools()
            .expect("global tool store should load");
        assert_eq!(loaded, store);
    }

    #[test]
    fn global_home_load_tools_reads_manifest_even_if_repo_tools_dir_missing() {
        let sandbox = TestDir::new();
        let global_home = GlobalHome::from_root(sandbox.path.join("global-home"));
        global_home.init().expect("global home should initialize");

        let store = ToolStore {
            packages: vec![InstalledPackage {
                name: "keep-me".to_string(),
                version: None,
                path: None,
            }],
        };
        global_home
            .save_tools(&store)
            .expect("global tool store should be saved");

        fs::remove_dir_all(global_home.tools_dir())
            .expect("global repo/tools dir should be removed");

        let loaded = global_home
            .load_tools()
            .expect("global tool store should still load from manifest");
        assert_eq!(loaded, store);
    }
}
