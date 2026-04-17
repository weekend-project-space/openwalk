# openwalk

Local-first Scheme runtime CLI with browser automation bindings.

AI FIRST file: YAML MD

## MVP

`openwalk run` and `openwalk exec` now support local Scheme files:

```bash
cargo run -- run browser-smoke
```

Scripts run through `scheme4r` and receive these globals:

- `openwalk-args`
- `openwalk-script-path`

Recommended script shape:

```scheme
(define (main args)
  ...)
```

`openwalk run ... -- foo bar` will pass `("foo" "bar")` into `main`.

Common runtime flags for both `openwalk run` and `openwalk exec`:

- `--session <name>` / `-s=<name>`: use a named browser session
- `--format <fmt>` / `-f=<fmt>`: output format, supports `yaml` (default), `md`, `json`

Examples:

```bash
cargo run -- exec tab-list -s=qa
cargo run -- exec tab-list -f=json
cargo run -- run bing-search --format md -- "Claude Code" 5
```

Browser state is persistent by default. Cookies, localStorage, and login sessions are stored in:

- `~/.openwalk/browser-profile/default`

Project-level metadata and declared tool dependencies now live in:

- `openwalk.json`

## Tool Metadata

Local Scheme tools can declare metadata with a `#| @meta ... |#` header:

```scheme
#| @meta
{
  "name": "bing-search",
  "description": "Bing 搜索并返回结构化结果",
  "args": [
    {
      "name": "query",
      "type": "string",
      "required": true,
      "description": "搜索关键词"
    }
  ],
  "returns": {
    "type": "object",
    "description": "{ query, count, results[] }"
  },
  "examples": [
    "openwalk run bing-search -- \"Claude Code\" 10"
  ],
  "domains": ["www.bing.com"],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["search", "bing"]
}
|#
```

You can inspect it with:

```bash
cargo run -- tool info bing-search
```

## Browser Host Functions

The runtime injects browser helpers backed by `chromiumoxide`.

Main groups:

- `browser-*`: `browser-open`, `browser-close`, `browser-version`
- `page-*`: `page-goto`, `page-back`, `page-forward`, `page-reload`, `page-title`, `page-url`, `page-content`, `page-screenshot`, `page-pdf`
- `element-*`: `element-click`, `element-double-click`, `element-fill`, `element-type`, `element-text`, `element-attr`, `element-wait`, `element-exists`
- `keyboard-*`: `keyboard-press`, `keyboard-type`, `keyboard-down`, `keyboard-up`
- `mouse-*`: `mouse-click`, `mouse-move`, `mouse-down`, `mouse-up`, `mouse-wheel`
- `touch-*`: `touch-tap`
- `js-*`: `js-eval`, `js-wait`
- `time-*`: `time-sleep`
- `device-*`: `device-viewport`, `device-user-agent`, `device-set-user-agent`
- `cookie-*`: `cookie-list`, `cookie-get`, `cookie-set`, `cookie-delete`, `cookie-clear`
- `localstorage-*`: `localstorage-get`, `localstorage-set`, `localstorage-remove`, `localstorage-clear`, `localstorage-list`
- `sessionstorage-*`: `sessionstorage-get`, `sessionstorage-set`, `sessionstorage-remove`, `sessionstorage-clear`, `sessionstorage-list`
- `tab-*`: `tab-list`, `tab-current`, `tab-new`, `tab-select`, `tab-close`, `tab-wait-new`
- `network-*`: `network-list`, `network-wait-response`, `network-response-body`
- `console-*`: `console-list`, `console-errors`, `console-clear`, `console-wait`
- `inspect-*`: `inspect-info`, `inspect-highlight`, `inspect-hide-highlight`, `inspect-pick`
- `tracing-*`: `tracing-start`, `tracing-stop`
- `cdp-*`: `cdp-call`

Scheme 侧统一使用 `领域-动作` 命名，不再提供旧的兼容别名。

Example:

```scheme
(define (main args)
  (browser-open "https://www.baidu.com")
  (element-wait "input[name='wd']")
  (element-fill "input[name='wd']" "hello openwalk")
  (keyboard-press "Enter")
  (page-wait-navigation)
  (list
    (page-title)
    (js-eval "document.querySelector(\"input[name='wd']\")?.value || ''")
    (page-url)))
```

Repo example:

```bash
cargo run -- run browser-smoke
```

Advanced example:

```bash
cargo run -- run browser-advanced
```

Open Baidu example:

Search Bing example:

```bash
cargo run -- run bing-search -- "Claude Code" 10
```

V2EX hot topics example:

```bash
cargo run -- run v2ex-hot
```

## Tool Management

`openwalk` 现在区分项目级和全局级工具管理：

- `openwalk tool add <package>` / `openwalk tool remove <package>`
  - 作用于项目根目录的 `openwalk.json -> tools`
- `openwalk tool install <package>` / `openwalk tool uninstall <package>`
  - 作用于全局 openwalk home
  - 安装时会在全局 `bin` 目录下生成一个同名 shim，内部转发到 `openwalk exec <package>`
  - 默认 shim 路径是 `~/.openwalk/bin/<package>`
- `openwalk tool info <tool>`
  - 读取本地 `.scm` 脚本头部的 `#| @meta ... |#` 元信息

全局 openwalk home 默认是：

- `~/.openwalk`

也可以通过下面的环境变量覆盖：

- `OPENWALK_HOME`

如果希望直接在 shell 里调用这些全局工具，把下面目录加入 `PATH`：

- 默认：`~/.openwalk/bin`
- 自定义 home 时：`$OPENWALK_HOME/bin`

## Browser Configuration

When Chromium auto-detection is not enough, these environment variables are supported:

- `OPENWALK_HOME`
- `OPENWALK_BROWSER_BIN`
- `OPENWALK_BROWSER_PROFILE_DIR`
- `OPENWALK_NO_SANDBOX`
- `OPENWALK_HEADLESS`
- `OPENWALK_HEADFUL`
