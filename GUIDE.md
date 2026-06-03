# OpenWalk GUIDE

OpenWalk is an AI-friendly, local-first RPA runtime CLI that also provides common browser automation functions.

```bash
openwalk <command>
```

## Installation

The recommended way is to install the release binary directly:

```bash
# Linux
curl -fsSL https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.sh | bash
```

```powershell
# Windows PowerShell
irm https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.ps1 | iex
```

By default, the script will:

- Download the `openwalk` binary for the current platform from GitHub Releases
- Install it to `~/.openwalk/bin` or `%USERPROFILE%\.openwalk\bin`
- Try to add that directory to the current user's `PATH`

Common options:

```bash
bash scripts/install.sh --version v0.1.0
bash scripts/install.sh --install-dir /usr/local/bin --no-path
```

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -Version v0.1.0
powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -InstallDir C:\tools\openwalk -NoPath
```

## Quick Start

```bash
# Show available tools
openwalk tool ls

# Run a workspace tool
openwalk exec hello-word OpenWalk

# Open a browser and use a named session
openwalk exec browser-open https://example.com -s=demo
openwalk exec tab-list -s=demo
openwalk exec browser-close -s=demo
```

More commonly used capabilities are available in [openwalkhub](https://github.com/weekend-project-space/openwalkhub).

## Core Rules

- `exec` can run built-in host functions, local `.scm` files, workspace tools, and global tools
- If `exec` does not find a target locally, it will try to pull the tool from the hub into the current project and then execute it
- Supported output formats are `text`, `yaml`, `md`, and `json`
- `tool ls` outputs a compact tool list by default, and `tool info` outputs a readable documentation page by default
- Browser state is persisted by default

## Core Commands

| Command          | Usage                                  | Description                                                 |
| ---------------- | -------------------------------------- | ----------------------------------------------------------- |
| `install`        | `openwalk install`                     | Install project tools declared in `openwalk.json -> tools`  |
| `exec`           | `openwalk exec <tool-or-script>`       | Execute a host function, script, workspace tool, or global tool |
| `tool ls`        | `openwalk tool ls [-f <fmt>]`          | Show the list of directly executable tools                  |
| `tool info`      | `openwalk tool info <tool> [-f <fmt>]` | Show usage and metadata for a tool                          |

## `exec`

| Command                  | Target                    | Resolution Order                                                                    |
| ------------------------ | ------------------------- | ----------------------------------------------------------------------------------- |
| `openwalk exec <target>` | Host function, script, tool | Local `.scm` -> workspace tool -> built-in host function -> global tool -> auto-pull into current project |

Examples:

```bash
# Workspace tool
openwalk exec hello-word -- OpenWalk

# Local script
openwalk exec ./demo.scm -- foo bar

# Built-in browser command
openwalk exec browser-open https://example.com
```

Note: `exec` is the single entry point for built-in tools, local scripts, workspace tools, global tools, and hub tools.

## Common Runtime Options

| Option                           | Description                                                      |
| -------------------------------- | ---------------------------------------------------------------- |
| `-s <name>` / `--session <name>` | Specify a browser session                                        |
| `-f <fmt>` / `--format <fmt>`    | Output format: `text`, `yaml`, `md`, `json`                     |
| `--`                             | Stop parsing OpenWalk runtime options and pass the remaining arguments to the script as-is |

If `OPENWALK_SESSION_NAME` is set, it is used as the default session. An explicit `-s` / `--session` flag takes precedence.

Examples:

```bash
openwalk exec hello-word -f=json -- OpenWalk
openwalk exec browser-open https://example.com -s=qa
openwalk exec ./demo.scm -- -f=json
```

## Tool List and Details

```bash
# Output the compact list by default
openwalk tool ls

# Return a structured object array
openwalk tool ls -f=json

# Show the documentation page for a built-in tool
openwalk tool info browser-open

# Show the documentation page for a workspace tool
openwalk tool info hello-word
```

## Common Browser Commands

| Command           | Usage                                  | Description            |
| ----------------- | -------------------------------------- | ---------------------- |
| `browser-open`    | `openwalk exec browser-open <url>`     | Open the browser and navigate |
| `page-goto`       | `openwalk exec page-goto <url>`        | Navigate the current page |
| `page-screenshot` | `openwalk exec page-screenshot <path>` | Take a page screenshot |
| `tab-list`        | `openwalk exec tab-list`               | List tabs              |
| `tab-new`         | `openwalk exec tab-new [url]`          | Create a new tab       |
| `tab-select`      | `openwalk exec tab-select <tab>`       | Switch tabs            |
| `tab-close`       | `openwalk exec tab-close [tab]`        | Close a tab            |
| `dialog-accept`   | `openwalk exec dialog-accept [text]`   | Accept the current JavaScript dialog |
| `dialog-dismiss`  | `openwalk exec dialog-dismiss`         | Dismiss the current JavaScript dialog |
| `browser-close`   | `openwalk exec browser-close`          | Close the browser session |

For a quick browsable capability overview:

```bash
openwalk tool ls
```

For a structured capability overview:

```bash
openwalk tool ls -f=json
```

## Browser Sessions

- Default profile: `~/.openwalk/browser-profile/default`
- Named session profile: `~/.openwalk/browser-profile/<session>`
- `tab-list`, `tab-new`, `tab-select`, and `tab-close` require an already opened browser session

Recommended flow:

```bash
openwalk exec browser-open https://example.com -s=qa
openwalk exec tab-list -s=qa
openwalk exec browser-close -s=qa
```

`browser-open` also supports:

```bash
openwalk exec browser-open https://example.com --headed
openwalk exec browser-open https://example.com --new-tab -s=qa
openwalk exec browser-open https://example.com --profile /tmp/openwalk-profile
```

## Workspace and Global Directories

Project-level:

- `openwalk.json`
- `.openwalk/tools/<package>/main.scm`

Global default directory: `~/.openwalk`

- `~/.openwalk/repo/tools/<package>/main.scm`
- `~/.openwalk/bin/<package>`
- `~/.openwalk/browser-profile/default`

## Scheme Tools

OpenWalk runs scripts through `scheme4r` and injects:

- `openwalk-args`
- `openwalk-script-path`
- `openwalk-script-meta`
- `openwalk-session-name`

Recommended script shape:

```scheme
(define (main args)
  ...)
```

Minimal example:

```scheme
(define (main args)
  (if (null? args)
      "hello world"
      (string-append "hello " (car args))))
```

Run locally:

```bash
openwalk exec ./demo.scm -- OpenWalk
```

Workspace tool path:

```text
.openwalk/tools/<tool-name>/main.scm
```

## Environment Variables

| Variable                       | Description                                        |
| ------------------------------ | -------------------------------------------------- |
| `OPENWALK_HOME`                | Override the global home directory, default is `~/.openwalk` |
| `OPENWALK_HUB_GIT_URL`         | Specify the tool hub repository                    |
| `OPENWALK_HUB_GIT_REF`         | Specify the tool hub branch or ref                 |
| `OPENWALK_SESSION_NAME`        | Provide the default browser session name when `-s` / `--session` is not passed |
| `OPENWALK_BROWSER_BIN`         | Specify the Chromium / Chrome executable           |
| `OPENWALK_BROWSER_PROFILE_DIR` | Override the default non-session profile           |
| `OPENWALK_NO_SANDBOX`          | Disable the browser sandbox when launching         |
| `OPENWALK_HEADLESS`            | Control non-session browser mode: truthy values keep headless, falsey values launch headed |

For non-session `browser-open`, `OPENWALK_HEADLESS=1` keeps the default headless mode, and `OPENWALK_HEADLESS=0` launches a headed browser. The `--headed` flag takes precedence over the environment variable.
