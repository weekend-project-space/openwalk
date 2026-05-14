# OpenWalk

Turn websites into reusable local commands.

OpenWalk is a local-first CLI that brings website capabilities, real-browser automation, and project tools together behind a single entry point. Search, trending feeds, page actions, and workflow orchestration no longer have to live as throwaway scripts. They become commands you can reuse, refine, and build on over time.

```bash
openwalk exec bing/search "Claude Code" 5
openwalk exec reddit/hot LocalLLaMA
openwalk exec v2ex/hot
openwalk exec openwalkhub/tools

openwalk exec browser-open https://news.ycombinator.com -s=demo
openwalk exec page-snapshot -s=demo
```

What you get back is not a pile of scattered text, but structured output that is much easier to keep processing:

```json
{
  "query": "Claude Code",
  "count": 5,
  "results": [
    {
      "index": 1,
      "title": "...",
      "url": "..."
    }
  ]
}
```

If a tool is not installed yet, `openwalk exec` can fetch it from the hub into your current project and run it immediately.

## Why You Will Want To Keep Using It

- Turn website capabilities into commands instead of one-off scripts
- Use a real browser for automation instead of stopping at offline scraping
- Stay local-first so your login state, files, and local environment are easier to reuse
- Friendly for humans and for AI agents
- Run ready-made tools right away, and grow custom tools into long-term team assets

## Get Started In 30 Seconds

### 1. Install

```bash
# Linux
curl -fsSL https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.sh | bash
```

```powershell
# Windows PowerShell
irm https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.ps1 | iex
```

### 2. Initialize the current project

```bash
openwalk init
```

### 3. Run your first command

```bash
openwalk exec bing/search "OpenAI" 5
openwalk exec reddit/hot LocalLLaMA
openwalk exec v2ex/hot
```

### 4. Open a real browser session

```bash
openwalk exec browser-open https://example.com -s=demo --headed
openwalk exec page-snapshot -s=demo
openwalk exec browser-close -s=demo
```

## One Entry Point For Three Kinds Of Capabilities

### Ready-made website commands

Turn common website actions into stable commands:

- `bing/search`
- `reddit/hot`
- `v2ex/hot`
- Plus more `search`, `hot`, `detail`, `comments`, and `download` style tools from the hub

These commands work well both for direct human use and for plugging into scripts, pipelines, and agent workflows.

### Built-in browser primitives

OpenWalk includes a set of browser capabilities you can call directly:

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

You can use these capabilities directly, or compose them into your own website workflows step by step.

### Project-level and global tools

OpenWalk is not only for running prebuilt commands. It also helps you turn capabilities into long-term reusable tools:

- Workspace tools
- Global tools
- Tools pulled from the hub
- Local `.scm` scripts

Temporary scripts do not have to stay temporary forever. They can gradually grow into tools your team actually reuses.

## What `openwalkhub` Currently Supports

OpenWalk can pull tools from `openwalkhub` into the current project at execution time. The table below was last updated on `2026-05-13`.

| Site / Category | Available Commands                                                                                  |
| --------------- | --------------------------------------------------------------------------------------------------- |
| `openwalkhub`   | `tools`                                                                                             |
| `bilibili`      | `comments` `feed` `history` `me` `opus` `popular` `ranking` `search` `trending` `user-opus` `video` |
| `bing`          | `search`                                                                                            |
| `csdn`          | `search`                                                                                            |
| `debug`         | `current-page` `open`                                                                               |
| `devto`         | `search`                                                                                            |
| `douban`        | `movie-top250` `search`                                                                             |
| `github`        | `fork` `issue-create` `issues` `me` `pr-create` `repo`                                              |
| `hackernews`    | `thread` `top`                                                                                      |
| `hupu`          | `hot`                                                                                               |
| `jike`          | `latest` `search` `tag` `topic`                                                                     |
| `linkedin`      | `profile` `search`                                                                                  |
| `linuxdo`       | `hot` `latest` `topic`                                                                              |
| `producthunt`   | `today`                                                                                             |
| `reddit`        | `context` `hot` `me` `posts` `search` `thread`                                                      |
| `stackoverflow` | `search`                                                                                            |
| `twitter`       | `bookmarks` `following` `for_you` `notifications` `search` `thread` `tweets` `user`                 |
| `v2ex`          | `hot` `latest` `topic`                                                                              |
| `weibo`         | `hot` `search`                                                                                      |
| `xiaohongshu`   | `note` `search`                                                                                     |
| `xiaoyuzhoufm`  | `episode` `podcast`                                                                                 |
| `youtube`       | `channel` `comments` `feed` `search` `transcript` `video`                                           |
| `zhihu`         | `hot` `question` `search`                                                                           |

Some of the best commands to show new users right away include:

- Trending and timelines: `reddit/hot` `v2ex/hot` `linuxdo/hot` `weibo/hot` `zhihu/hot` `hackernews/top` `producthunt/today`
- Search: `bing/search` `reddit/search` `twitter/search` `youtube/search` `zhihu/search` `stackoverflow/search`
- Detail and content reading: `v2ex/topic` `zhihu/question` `hackernews/thread` `reddit/thread` `youtube/video` `youtube/transcript`
- Platform and account data: `github/me` `github/issues` `twitter/bookmarks` `twitter/notifications` `linkedin/profile`

## For Humans And For AI Agents

### For humans

When you want to turn repetitive website actions into stable commands, OpenWalk feels very natural:

- `openwalk exec openwalkhub/tools` to see what tools are available on the hub
- `openwalk tool list` to see what you can run right now
- `openwalk tool info bing/search` to check how a specific tool works
- Run ready-made website commands directly instead of reopening sites and clicking through them every time

### For AI agents

If you want a stable execution surface for an AI agent, OpenWalk is a natural fit:

- Agents can call ready-made commands instead of rediscovering websites from scratch every time
- Browser capabilities can reuse the real local environment
- Structured output is easier to feed into downstream reasoning, extraction, and orchestration
- A website flow explored once can become a command that runs directly next time

## Continue From Here

```bash
openwalk exec openwalkhub/tools
openwalk tool list
openwalk tool info bing/search
openwalk tool info browser-open
```

For the full manual, installation details, tool authoring, environment variables, and directory structure, see [GUIDE.md](./GUIDE.md).
