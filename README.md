# OpenWalk

Turn websites into reusable local commands.

OpenWalk is a local-first CLI for running website tools, built-in browser automation, and local Scheme tools behind one entry point:

```bash
openwalk exec <tool> [args...]
```

Use it when a website workflow should stop being a one-off script and become something you can run, inspect, reuse, and hand to an agent.

```bash
openwalk exec openwalkhub/tools
openwalk tool ls

openwalk exec bing/search "Claude Code" 5
openwalk exec reddit/hot LocalLLaMA
openwalk exec v2ex/hot

openwalk exec browser-open https://news.ycombinator.com -s=demo --new-tab --headed
openwalk exec page-snapshot -s=demo
openwalk exec browser-close -s=demo
```

If a hub tool is not installed yet, `openwalk exec` can fetch it into the current project and run it immediately.

## Why OpenWalk

- Turn website capabilities into commands instead of throwaway scripts
- Use a real local browser when scraping is not enough
- Reuse local login state, browser profiles, files, and environment
- Give humans and AI agents the same stable execution surface
- Keep tool output structured so it is easier to inspect and pipe into follow-up work

## Install

```bash
# Linux
curl -fsSL https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.sh | bash
```

```powershell
# Windows PowerShell
iex "& { $(irm https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.ps1) }"
```

## Quick Start

List tools that are available locally:

```bash
openwalk tool ls
```

Discover hub tools:

```bash
openwalk exec openwalkhub/tools
```

Run a website command:

```bash
openwalk exec bing/search "OpenAI" 5
openwalk exec reddit/hot LocalLLaMA
openwalk exec v2ex/hot
```

Inspect a tool before running it:

```bash
openwalk exec browser-open --help
openwalk tool info browser-open
openwalk tool info browser-open --format=json
```

`openwalk exec <tool> --help` and `openwalk tool info <tool>` show the same human-readable help by default. Use `--format=json` with `tool info` when you need structured metadata.

## Browser Automation

Open a browser session, run page commands against it, then close it:

```bash
openwalk exec browser-open https://example.com -s=demo --headed
openwalk exec page-snapshot -s=demo
openwalk exec browser-close -s=demo
```

`browser-open` supports:

```bash
openwalk exec browser-open https://example.com -s=demo
openwalk exec browser-open https://example.com -s=demo --headed
openwalk exec browser-open https://example.com -s=demo --new-tab
openwalk exec browser-open https://example.com -s=demo --profile /tmp/openwalk-profile
```

The default browser mode is headless. Use `--headed` when you want to see the browser UI.

Useful built-in browser tools include:

- `browser-open`
- `page-goto`
- `page-snapshot`
- `page-screenshot`
- `element-click`
- `tab-list`
- `tab-new`
- `tab-select`
- `tab-close`
- `browser-close`

## Core Commands

| Command       | Example                                          | Purpose                                                               |
| ------------- | ------------------------------------------------ | --------------------------------------------------------------------- |
| `exec`        | `openwalk exec browser-open https://example.com` | Run a built-in tool, local script, workspace/global tool, or hub tool |
| `install`     | `openwalk install`                               | Install tools declared by the current project                         |
| `tool ls`     | `openwalk tool ls`                               | List directly runnable tools                                          |
| `tool info`   | `openwalk tool info browser-open`                | Show usage, arguments, options, returns, and examples                 |
| `exec --help` | `openwalk exec browser-open --help`              | Show tool help without running the tool                               |

`exec` is the main path. It resolves tools in this order: local script, workspace tool, built-in host function, global tool, then hub auto-install into the current project.

## Hub Tools

OpenWalk can pull reusable website tools from `openwalkhub`. Start with:

```bash
openwalk exec openwalkhub/tools
```

Examples of useful hub-style commands include:

- Search: `bing/search`, `reddit/search`, `youtube/search`, `zhihu/search`
- Timelines and trends: `reddit/hot`, `v2ex/hot`, `linuxdo/hot`, `hackernews/top`
- Detail pages: `v2ex/topic`, `zhihu/question`, `hackernews/thread`, `reddit/thread`
- Account or platform workflows: `github/issues`, `twitter/bookmarks`, `linkedin/profile`

Availability changes over time, so prefer `openwalk exec openwalkhub/tools` for the current list.

## For Humans And Agents

For humans, OpenWalk turns repeated website work into commands you can remember and improve.

For agents, OpenWalk provides a stable local action surface:

- Tool help is discoverable with `openwalk exec <tool> --help`
- Tool metadata is available with `openwalk tool info <tool> --format=json`
- Browser commands reuse the real local environment
- Structured outputs are easier to feed into reasoning, extraction, and orchestration

## Continue From Here

```bash
openwalk exec openwalkhub/tools
openwalk tool ls
openwalk exec browser-open --help
openwalk tool info bing/search
```

For the full manual, installation details, tool authoring, environment variables, and directory structure, see [GUIDE.md](./GUIDE.md).
