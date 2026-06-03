use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

// Parsing stays intentionally thin in this module so command behavior can evolve in `app.rs`
// without coupling execution logic to Clap-specific details.
#[derive(Debug, Parser)]
#[command(
    name = "openwalk",
    version,
    about = "Turn websites and browsers into reusable local tools.",
    after_help = "Discover tools:\n  openwalk tool ls                 List workspace, global, and kit tools\n  openwalk tool ls --all           Include built-in tools\n  openwalk tool info <tool>        Show usage, arguments, options, returns, and examples\n  openwalk exec <tool> --help      Show tool help without running it\n\nRemote tools:\n  openwalk exec hub/tools          Discover tools from the remote hub\n  openwalk exec v2ex/hot           Pull and run a remote tool by ref\n\nExamples:\n  openwalk tool ls\n  openwalk tool info browser-open\n  openwalk exec browser-open --help\n  openwalk exec browser-open https://example.com -s=demo --headed\n\n",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Top-level command selected by the user.
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    // Init(InitArgs),
    /// Install tools declared in openwalk.json
    Install(ProjectInstallArgs),
    /// Execute a built-in tool, script, or installed tool
    Exec(ToolExecArgs),
    /// List and inspect available tools
    #[command(
        after_help = "Examples:\n  openwalk tool ls\n  openwalk tool ls --all\n  openwalk tool ls --source kit\n  openwalk tool info browser-open\n\n"
    )]
    Tool {
        #[command(subcommand)]
        command: ToolCommand,
    },
    #[command(hide = true)]
    Daemon(DaemonArgs),
}

#[derive(Debug, Args)]
/// Flags accepted by `openwalk init`.
pub struct InitArgs {
    /// Override the package name written to openwalk.json.
    #[arg(long)]
    pub name: Option<String>,

    /// Pre-populate the tools map with comma-separated names, e.g. --tools=v2ex/hot,bing-search
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
/// Flags accepted by `openwalk install`.
pub struct ProjectInstallArgs {}

#[derive(Debug, Args)]
/// Shared argument shape for commands that dispatch a single tool invocation.
#[command(disable_help_flag = true)]
pub struct ToolExecArgs {
    /// Scheme script path or tool name, for example ./demo.scm
    pub tool: Option<String>,

    /// Show help for the exec command itself. Use `openwalk exec <tool> --help` for tool help.
    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
    pub help: bool,

    // #[arg(short = 's', long = "session", default_value = "default")]
    // pub session: String,

    // #[arg(short = 'f', long = "format", default_value = "yaml")]
    // pub format: String,
    /// Additional arguments passed to the tool.
    // Keep the remainder untouched so tool-specific flags are not parsed as CLI flags.
    #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

pub fn render_exec_help() -> &'static str {
    "Execute a built-in tool, local script, workspace/global tool, or kit tool.\n\n\
Usage:\n  openwalk exec <tool> [args...]\n\n\
Arguments:\n  <tool>      Built-in tool name, tool ref, or local .scm path\n  [args...]   Arguments and tool-specific options passed to the tool\n\n\
Common options after <tool>:\n  -s, --session <name>       Browser session name\n  -f, --format <format>      Output format: text, yaml, md, or json\n  --all                      Include execution metadata in the output\n  --help                     Show help for the selected tool\n\n\
Discover tools:\n  openwalk tool ls           List workspace, global, and kit tools\n  openwalk tool ls --all     Include built-in tools\n  openwalk tool info <tool>  Show usage, arguments, options, returns, and examples\n\n\
Remote tools:\n  openwalk exec hub/tools    Discover tools from the remote hub\n  openwalk exec v2ex/hot     Pull and run a remote tool by ref\n\n\
Examples:\n  openwalk exec browser-open https://example.com -s=demo --headed\n  openwalk exec page-snapshot -s=demo --format=json\n  openwalk exec browser-open --help\n  openwalk exec search \"rust mcp\"\n"
}

#[derive(Debug, Args)]
pub struct DaemonArgs {
    #[arg(long)]
    pub session: String,
    #[arg(long)]
    pub port: u16,
    #[arg(long)]
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ToolListSourceFilter {
    Workspace,
    Global,
    Kit,
    Builtin,
}

#[derive(Debug, Subcommand)]
pub enum ToolCommand {
    // /// Add a tool package into the current workspace.
    // Add {
    //     /// Tool package name, for example browser-tools
    //     package: String,
    // },
    // /// Remove a tool package from the current workspace.
    // Remove {
    //     /// Tool package name, for example browser-tools
    //     package: String,
    // },
    // /// Install a tool package into the global openwalk home and create a runnable shim.
    // Install {
    //     /// Tool package name, for example browser-tools
    //     package: String,
    // },
    // /// Uninstall a tool package from the global openwalk home and remove its shim.
    // Uninstall {
    //     /// Tool package name, for example browser-tools
    //     package: String,
    // },
    /// List runnable workspace, global, and kit tools.
    #[command(
        after_help = "Notes:\n  Built-in tools are hidden by default. Use `--all` to include them.\n  Use `--source` to filter by workspace, global, kit, or builtin.\n\nExamples:\n  openwalk tool ls\n  openwalk tool ls --all\n  openwalk tool ls --source kit\n  openwalk tool ls --format=json\n\n"
    )]
    Ls {
        /// Output format: text (default compact list), yaml, md, or json.
        #[arg(short = 'f', long = "format", default_value = "text")]
        format: String,

        /// Filter tools by source.
        #[arg(long = "source", value_enum)]
        source: Option<ToolListSourceFilter>,

        /// Include built-in host functions.
        #[arg(long)]
        all: bool,
    },
    /// Show usage and metadata for a built-in tool, installed tool, or local Scheme script.
    #[command(
        after_help = "Examples:\n  openwalk tool info browser-open\n  openwalk tool info search\n  openwalk tool info ./demo.scm\n  openwalk tool info browser-open --format=json\n\n"
    )]
    Info {
        /// Built-in tool name, tool ref, or local .scm file path.
        tool: String,
        /// Output format: yaml (default), md, or json.
        #[arg(short = 'f', long = "format", default_value = "yaml")]
        format: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any())]
    #[test]
    fn parses_init_with_name_tools_and_force() {
        let cli = Cli::try_parse_from([
            "openwalk",
            "init",
            "--name=my-walk",
            "--tools=v2ex/hot,bing-search",
            "--force",
            "--format=json",
        ])
        .expect("init command should parse");

        match cli.command {
            Command::Init(args) => {
                assert_eq!(args.name.as_deref(), Some("my-walk"));
                assert_eq!(args.tools, vec!["v2ex/hot", "bing-search"]);
                assert!(args.force);
                assert_eq!(args.format, "json");
            }
            other => panic!("expected init command, got {other:?}"),
        }
    }

    #[cfg(any())]
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

    #[cfg(any())]
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
    fn parses_install_without_packages() {
        let cli = Cli::try_parse_from(["openwalk", "install"])
            .expect("install without package names should parse");

        match cli.command {
            Command::Install(_) => {}
            other => panic!("expected install command, got {other:?}"),
        }
    }

    #[cfg(any())]
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
                assert_eq!(args.tool.as_deref(), Some("browser-open"));
                assert!(!args.help);
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
                assert_eq!(args.tool.as_deref(), Some("browser-open"));
                assert!(!args.help);
                assert_eq!(args.args, vec!["-s=parallel-a", "https://example.com"]);
            }
            other => panic!("expected exec command, got {other:?}"),
        }
    }

    #[test]
    fn parses_exec_command_with_tool_help_arg() {
        let cli = Cli::try_parse_from(["openwalk", "exec", "browser-open", "--help"])
            .expect("exec command should preserve tool help as an argument");

        match cli.command {
            Command::Exec(args) => {
                assert_eq!(args.tool.as_deref(), Some("browser-open"));
                assert!(args.help);
                assert!(args.args.is_empty());
            }
            other => panic!("expected exec command, got {other:?}"),
        }
    }

    #[test]
    fn parses_exec_command_help_before_tool() {
        let cli = Cli::try_parse_from(["openwalk", "exec", "--help"])
            .expect("exec command help should parse");

        match cli.command {
            Command::Exec(args) => {
                assert_eq!(args.tool, None);
                assert!(args.help);
                assert!(args.args.is_empty());
            }
            other => panic!("expected exec command, got {other:?}"),
        }
    }

    #[test]
    fn parses_exec_command_without_tool_for_app_level_help() {
        let cli =
            Cli::try_parse_from(["openwalk", "exec"]).expect("bare exec should parse cleanly");

        match cli.command {
            Command::Exec(args) => {
                assert_eq!(args.tool, None);
                assert!(!args.help);
                assert!(args.args.is_empty());
            }
            other => panic!("expected exec command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_list_format_flag() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "ls", "--format=json"])
            .expect("tool list should parse");

        match cli.command {
            Command::Tool {
                command:
                    ToolCommand::Ls {
                        format,
                        source,
                        all,
                    },
            } => {
                assert_eq!(format, "json");
                assert_eq!(source, None);
                assert!(!all);
            }
            other => panic!("expected tool list command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_list_source_filter() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "ls", "--source=kit"])
            .expect("tool list source filter should parse");

        match cli.command {
            Command::Tool {
                command:
                    ToolCommand::Ls {
                        format,
                        source,
                        all,
                    },
            } => {
                assert_eq!(format, "text");
                assert_eq!(source, Some(ToolListSourceFilter::Kit));
                assert!(!all);
            }
            other => panic!("expected tool list command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_list_all_flag() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "ls", "--all"])
            .expect("tool list all flag should parse");

        match cli.command {
            Command::Tool {
                command:
                    ToolCommand::Ls {
                        format,
                        source,
                        all,
                    },
            } => {
                assert_eq!(format, "text");
                assert_eq!(source, None);
                assert!(all);
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
                command: ToolCommand::Info { tool, format },
            } => {
                assert_eq!(tool, "bing-search");
                assert_eq!(format, "yaml");
            }
            other => panic!("expected tool info command, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_info_format_flag() {
        let cli = Cli::try_parse_from(["openwalk", "tool", "info", "./demo.scm", "-f=json"])
            .expect("tool info should parse");

        match cli.command {
            Command::Tool {
                command: ToolCommand::Info { tool, format },
            } => {
                assert_eq!(tool, "./demo.scm");
                assert_eq!(format, "json");
            }
            other => panic!("expected tool info command, got {other:?}"),
        }
    }

    #[cfg(any())]
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

    #[cfg(any())]
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

    #[cfg(any())]
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

    #[cfg(any())]
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
