use clap::{Args, Parser, Subcommand};

// Parsing stays intentionally thin in this module so command behavior can evolve in `app.rs`
// without coupling execution logic to Clap-specific details.
#[derive(Debug, Parser)]
#[command(
    name = "openwalk",
    version,
    about = "Local-first Scheme runtime CLI",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Top-level command selected by the user.
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize an openwalk workspace in the current directory.
    Init(InitArgs),
    /// Run a workspace tool name or local Scheme script file. Built-in host functions use `exec`.
    Run(ToolExecArgs),
    /// Execute a built-in host function, local Scheme script file, or tool entry.
    Exec(ToolExecArgs),
    /// Manage locally available tools.
    Tool {
        #[command(subcommand)]
        command: ToolCommand,
    },
}

#[derive(Debug, Args)]
/// Flags accepted by `openwalk init`.
pub struct InitArgs {
    /// Override the package name written to openwalk.json.
    #[arg(long)]
    pub name: Option<String>,

    /// Pre-populate the tools map with comma-separated names, e.g. --tools=v2ex-hot,bing-search
    #[arg(long, value_delimiter = ',')]
    pub tools: Vec<String>,

    /// Overwrite an existing openwalk.json (the previous file is backed up to openwalk.json.bak).
    #[arg(long)]
    pub force: bool,

    /// Output format: yaml (default), md, or json.
    #[arg(short = 'f', long = "format", default_value = "yaml")]
    pub format: String,
}

#[derive(Debug, Args)]
/// Shared argument shape for commands that dispatch a single tool invocation.
pub struct ToolExecArgs {
    /// Scheme script path or tool name, for example ./demo.scm
    pub tool: String,
    /// Additional arguments passed to the tool.
    // Keep the remainder untouched so tool-specific flags are not parsed as CLI flags.
    #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum ToolCommand {
    /// Add a tool package into the current workspace.
    Add {
        /// Tool package name, for example browser-tools
        package: String,
    },
    /// Remove a tool package from the current workspace.
    Remove {
        /// Tool package name, for example browser-tools
        package: String,
    },
    /// Install a tool package into the global openwalk home and create a runnable shim.
    Install {
        /// Tool package name, for example browser-tools
        package: String,
    },
    /// Uninstall a tool package from the global openwalk home and remove its shim.
    Uninstall {
        /// Tool package name, for example browser-tools
        package: String,
    },
    /// List built-in host functions and installed tools.
    List {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show metadata for a built-in host function or local Scheme tool script.
    Info {
        /// Workspace tool name or local .scm file path.
        tool: String,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_with_name_tools_and_force() {
        let cli = Cli::try_parse_from([
            "openwalk",
            "init",
            "--name=my-walk",
            "--tools=v2ex-hot,bing-search",
            "--force",
            "--format=json",
        ])
        .expect("init command should parse");

        match cli.command {
            Command::Init(args) => {
                assert_eq!(args.name.as_deref(), Some("my-walk"));
                assert_eq!(args.tools, vec!["v2ex-hot", "bing-search"]);
                assert!(args.force);
                assert_eq!(args.format, "json");
            }
            other => panic!("expected init command, got {other:?}"),
        }
    }

    #[test]
    fn parses_init_without_flags_uses_defaults() {
        let cli = Cli::try_parse_from(["openwalk", "init"]).expect("bare init should parse");

        match cli.command {
            Command::Init(args) => {
                assert!(args.name.is_none());
                assert!(args.tools.is_empty());
                assert!(!args.force);
                assert_eq!(args.format, "yaml");
            }
            other => panic!("expected init command, got {other:?}"),
        }
    }

    #[test]
    fn parses_run_command_with_trailing_args() {
        let cli = Cli::try_parse_from([
            "openwalk",
            "run",
            "browser.open",
            "https://example.com",
            "--headless",
        ])
        .expect("run command should parse");

        match cli.command {
            Command::Run(args) => {
                assert_eq!(args.tool, "browser.open");
                assert_eq!(args.args, vec!["https://example.com", "--headless"]);
            }
            other => panic!("expected run command, got {other:?}"),
        }
    }

    #[test]
    fn parses_run_command_with_session() {
        let cli = Cli::try_parse_from(["openwalk", "run", "baidu-search", "-s=default", "rust"])
            .expect("run command should keep trailing args untouched");

        match cli.command {
            Command::Run(args) => {
                assert_eq!(args.tool, "baidu-search");
                assert_eq!(args.args, vec!["-s=default", "rust"]);
            }
            other => panic!("expected run command, got {other:?}"),
        }
    }

    #[test]
    fn parses_exec_command_with_browser_launch_options() {
        let cli = Cli::try_parse_from([
            "openwalk",
            "exec",
            "browser-open",
            "https://example.com",
            "--headed",
            "--profile=/tmp/openwalk-profile",
            "-s=qa",
        ])
        .expect("exec command with launch options should parse");

        match cli.command {
            Command::Exec(args) => {
                assert_eq!(args.tool, "browser-open");
                assert_eq!(
                    args.args,
                    vec![
                        "https://example.com",
                        "--headed",
                        "--profile=/tmp/openwalk-profile",
                        "-s=qa",
                    ]
                );
            }
            other => panic!("expected exec command, got {other:?}"),
        }
    }

    #[test]
    fn parses_exec_command_with_short_session_equals_form() {
        let cli = Cli::try_parse_from([
            "openwalk",
            "exec",
            "browser-open",
            "-s=parallel-a",
            "https://example.com",
        ])
        .expect("exec command with -s=<name> should parse");

        match cli.command {
            Command::Exec(args) => {
                assert_eq!(args.tool, "browser-open");
                assert_eq!(args.args, vec!["-s=parallel-a", "https://example.com"]);
            }
            other => panic!("expected exec command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_list_json_flag() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "list", "--json"])
            .expect("tool list should parse");

        match cli.command {
            Command::Tool {
                command: ToolCommand::List { json },
            } => {
                assert!(json);
            }
            other => panic!("expected tool list command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_info_target() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "info", "bing-search"])
            .expect("tool info should parse");

        match cli.command {
            Command::Tool {
                command: ToolCommand::Info { tool, json },
            } => {
                assert_eq!(tool, "bing-search");
                assert!(!json);
            }
            other => panic!("expected tool info command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_info_json_flag() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "info", "./demo.scm", "--json"])
            .expect("tool info should parse");

        match cli.command {
            Command::Tool {
                command: ToolCommand::Info { tool, json },
            } => {
                assert_eq!(tool, "./demo.scm");
                assert!(json);
            }
            other => panic!("expected tool info command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_add_package() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "add", "browser-tools"])
            .expect("tool add should parse");

        match cli.command {
            Command::Tool {
                command: ToolCommand::Add { package },
            } => {
                assert_eq!(package, "browser-tools");
            }
            other => panic!("expected tool add command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_remove_package() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "remove", "browser-tools"])
            .expect("tool remove should parse");

        match cli.command {
            Command::Tool {
                command: ToolCommand::Remove { package },
            } => {
                assert_eq!(package, "browser-tools");
            }
            other => panic!("expected tool remove command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_install_package() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "install", "browser-tools"])
            .expect("tool install should parse");

        match cli.command {
            Command::Tool {
                command: ToolCommand::Install { package },
            } => {
                assert_eq!(package, "browser-tools");
            }
            other => panic!("expected tool install command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_uninstall_package() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "uninstall", "browser-tools"])
            .expect("tool uninstall should parse");

        match cli.command {
            Command::Tool {
                command: ToolCommand::Uninstall { package },
            } => {
                assert_eq!(package, "browser-tools");
            }
            other => panic!("expected tool uninstall command, got {other:?}"),
        }
    }
}
