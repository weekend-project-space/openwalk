# OpenWalk 中文说明

OpenWalk 是一个 local-first 的 Scheme runtime CLI，用来把本地 Scheme 脚本、浏览器自动化能力和工具分发结合在一起。

这份文档以当前仓库里的实际实现为准，而不是只根据设计稿整理。如果静态文档和程序行为不一致，请优先以这两个命令的输出为准：

```bash
cargo run -- tool list --json
cargo run -- tool info <tool> --json
```

下面的示例默认都在仓库根目录运行，并使用 `cargo run -- ...`。如果你已经把二进制安装到了环境里，可以把前缀替换成 `openwalk`。

## 它能做什么

- 用 Scheme 编写本地自动化工具
- 直接执行内建浏览器 host function
- 按项目级或全局级安装工具
- 在 `exec` 未命中目标时，从 hub 自动拉取工具到当前项目后再执行
- 以 YAML、Markdown 或 JSON 输出结果，方便给人看，也方便给 AI / Agent 消费

## 核心概念

- `run`
  运行本地 `.scm` 文件或当前工作区里的 Scheme tool
- `exec`
  执行内建 host function、本地 `.scm` 文件、工作区 tool、全局 tool；如果都未命中，会尝试从 hub 拉取到当前项目后再执行
- 工作区
  当前项目目录，包含 `openwalk.json` 和 `.openwalk/`
- 全局 home
  默认是 `~/.openwalk`，用于存放全局安装的工具、浏览器 profile、session 运行状态和 shim

最重要的边界是：

- `openwalk run` 不能直接执行内建 host function，例如 `browser-open`
- `openwalk exec` 可以执行内建 host function，也可以执行脚本和工具

## 从源码运行

运行当前仓库需要：

- Rust 和 Cargo
- 可用的 Chromium / Chrome 浏览器

如果浏览器自动探测失败，可以设置：

```bash
export OPENWALK_BROWSER_BIN=/path/to/chrome
```

如果你想先编译成独立二进制：

```bash
cargo build --release
./target/release/openwalk tool list
```

## 30 秒上手

### 1. 初始化工作区

```bash
cargo run -- init
```

它会在当前目录创建：

- `openwalk.json`
- `.openwalk/`
- `.openwalk/tools/`

一个真实的初始化输出大致像这样：

```yaml
created:
  manifest: true
  root: true
  tool_dir: true
manifest: /path/to/project/openwalk.json
mode: init
overwritten_manifest: false
status: initialized
workspace: /path/to/project
```

如果你需要覆盖已有的 `openwalk.json`，可以使用：

```bash
cargo run -- init --force
```

原文件会被备份成 `openwalk.json.bak`。

### 2. 查看当前可用工具

```bash
cargo run -- tool list
```

或者输出 JSON：

```bash
cargo run -- tool list --json
```

当前这个仓库里自带了几个工作区示例工具：

- `hello-word`
- `bing-search`
- `v2ex-hot`

### 3. 跑一个最简单的 Scheme tool

```bash
cargo run -- run hello-word -- OpenWalk
```

实际输出类似：

```yaml
args:
  - OpenWalk
mode: run
result: hello OpenWalk
script: /path/to/.openwalk/tools/hello-word/main.scm
status: executed
```

如果你更想拿到 JSON：

```bash
cargo run -- run hello-word -f=json -- OpenWalk
```

### 4. 看某个工具的元信息

```bash
cargo run -- tool info hello-word
cargo run -- tool info bing-search --json
```

`tool info` 既可以接工作区 tool 名，也可以接本地 `.scm` 文件路径。

## `run` 和 `exec` 的区别

| 命令                     | 适合场景                                         | 解析顺序                                                                                    |
| ------------------------ | ------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `openwalk run <target>`  | 运行本地 Scheme 文件或工作区 tool                | 本地 `.scm` 路径 -> 工作区 tool                                                             |
| `openwalk exec <target>` | 执行 host function、脚本、工作区 tool、全局 tool | 本地 `.scm` 路径 -> 工作区 tool -> 内建 host function -> 全局 tool -> 从 hub 拉取到当前项目 |

几个典型例子：

```bash
# 运行工作区 tool
cargo run -- run hello-word -- OpenWalk

# 运行本地 Scheme 文件
cargo run -- run ./demo.scm -- foo bar

# 执行内建浏览器 host function
cargo run -- exec browser-open https://example.com

# 执行工作区 tool（exec 也可以）
cargo run -- exec bing-search -- "Claude Code" 5
```

如果你写了下面这样的命令：

```bash
cargo run -- run browser-open https://example.com
```

它会失败，因为 `browser-open` 是内建 host function，不是 `run` 的目标类型。

## 常用命令

### 工作区命令

```bash
cargo run -- init
cargo run -- install
cargo run -- run <tool-or-script>
cargo run -- exec <tool-or-script>
```

说明：

- `init` 初始化当前目录的 OpenWalk 工作区
- `install` 按 `openwalk.json -> tools` 中声明的内容安装到 `.openwalk/tools/`
- `run` 只跑本地 `.scm` 或工作区 tool
- `exec` 的能力面更大，也会在未命中时自动拉取工具

### 工具管理

```bash
cargo run -- tool add <package>
cargo run -- tool remove <package>
cargo run -- tool install <package>
cargo run -- tool uninstall <package>
cargo run -- tool list
cargo run -- tool info <tool>
```

说明：

- `tool add <package>`
  把工具安装到当前项目的 `.openwalk/tools/<package>/`
- `tool remove <package>`
  从当前项目移除工具目录，并更新 `openwalk.json`
- `tool install <package>`
  全局安装到 `~/.openwalk/repo/tools/<package>/`
- `tool uninstall <package>`
  从全局 home 卸载工具，并删除 shim
- `tool list`
  查看当前内建 host function、工作区 tool、声明的项目依赖、全局 package
- `tool info`
  查看 host function、工作区 tool、本地 `.scm` 或全局 tool 的元信息

补充说明：

- `openwalk install` 只会安装 `openwalk.json` 已经声明的 tools，不会自动新增依赖
- `tool add` 和“未命中的 `exec` 自动拉取”都会在必要时自动补齐 `.openwalk/` 和 `openwalk.json`
- 全局安装后会在 `~/.openwalk/bin/<package>` 生成同名 shim，内部转发到 `openwalk exec <package>`

## 运行参数

`run` 和 `exec` 都支持这些公共参数：

- `-s <name>`
- `--session <name>`
- `-s=<name>`
- `--session=<name>`
- `-f <fmt>`
- `--format <fmt>`
- `-f=<fmt>`
- `--format=<fmt>`

其中：

- `session` 用于指定浏览器会话名
- `format` 支持 `yaml`、`md`、`json`

示例：

```bash
cargo run -- exec browser-list -s=qa
cargo run -- run hello-word -f=json -- OpenWalk
cargo run -- run bing-search --format md -- "Claude Code" 5
```

`--` 的含义是停止解析 OpenWalk 的公共运行参数，后面的内容原样传给脚本的 `main`。例如：

```bash
cargo run -- run hello-word -- -f=json
```

这里 `-f=json` 不会被当成输出格式，而会作为普通字符串参数传给 Scheme 脚本。

## 浏览器自动化

OpenWalk 的浏览器能力来自内建 host function。它们既可以通过 `exec` 单独调用，也可以在 Scheme 脚本里直接调用。

常见分组包括：

- `browser-*`
- `page-*`
- `element-*`
- `keyboard-*`
- `mouse-*`
- `touch-*`
- `js-*`
- `cookie-*`
- `localstorage-*`
- `sessionstorage-*`
- `tab-*`
- `network-*`
- `console-*`
- `inspect-*`
- `tracing-*`
- `cdp-*`

几个常见例子：

```bash
cargo run -- exec browser-open https://www.baidu.com -s=demo
cargo run -- exec page-screenshot /tmp/example.png -s=demo
cargo run -- exec tab-list -s=demo
cargo run -- exec browser-close -s=demo
```

在 Scheme 里也可以这样写：

```scheme
(define (main args)
  (browser-open "https://www.baidu.com")
  (js-wait "Boolean(document.querySelector(\"input[name='wd']\"))")
  (element-fill "input[name='wd']" "hello openwalk")
  (keyboard-press "Enter")
  (page-wait-navigation)
  (js-eval "({ title: document.title, url: location.href })"))
```

如果你想看当前实现实际暴露了哪些 host function，最稳的方式是：

```bash
cargo run -- tool list --json
```

## 浏览器会话与持久化

浏览器状态默认是持久化的，cookies、localStorage 和登录态都会写入 profile 目录。

默认规则：

- 不指定 `-s/--session` 时，使用默认 profile
- 默认 profile 目录：`~/.openwalk/browser-profile/default`
- 指定 `-s=qa` 这类会话名时，会使用对应会话 profile
- 会话 profile 默认目录：`~/.openwalk/browser-profile/<session>`

典型用法：

```bash
cargo run -- exec browser-open https://example.com -s=qa
cargo run -- exec tab-list -s=qa
cargo run -- exec browser-close -s=qa
```

需要注意：

- `tab-list`、`tab-new`、`tab-select`、`tab-close` 会附着到已有 session，不会帮你自动创建浏览器
- 最稳妥的顺序通常是先 `browser-open -s=<name>`，再执行其他 `tab-*` 指令

`browser-open` 还有两个额外参数：

```bash
cargo run -- exec browser-open https://example.com --headed
cargo run -- exec browser-open https://example.com --profile /tmp/openwalk-profile
cargo run -- exec browser-open https://example.com -s=qa --headed
```

支持的附加参数只有：

- `--headed`
- `--profile <path>`
- `--profile=<path>`

## 工作区目录结构

项目级目录：

- `openwalk.json`
  项目清单，记录 `package` 信息和声明的 `tools`
- `.openwalk/tools/<package>/main.scm`
  当前项目已安装的工具入口

全局目录，默认在 `~/.openwalk`：

- `~/.openwalk/openwalk.json`
  全局工具清单
- `~/.openwalk/repo/tools/<package>/main.scm`
  全局安装的工具
- `~/.openwalk/bin/<package>`
  可直接执行的 shim
- `~/.openwalk/browser-profile/default`
  默认浏览器 profile
- `~/.openwalk/run/browser/<session>/`
  浏览器 session 的运行状态

## 编写一个 Scheme tool

OpenWalk 通过 `scheme4r` 运行脚本。脚本里会自动注入两个全局变量：

- `openwalk-args`
- `openwalk-script-path`

推荐的脚本形状是：

```scheme
(define (main args)
  ...)
```

`openwalk run ... -- foo bar` 会把 `("foo" "bar")` 传进 `main`。

一个最小的工具例子：

```scheme
#| @meta
{
  "name": "hello-word",
  "description": "返回一个简单的问候语，适合验证 Scheme tool 是否工作正常",
  "args": [
    {
      "name": "name",
      "type": "string",
      "required": false,
      "default": "world",
      "description": "可选的人名或目标词，默认 world"
    }
  ],
  "returns": {
    "type": "string",
    "description": "hello <name> 格式的问候语"
  },
  "examples": [
    "openwalk run hello-word",
    "openwalk run hello-word -- OpenWalk"
  ],
  "domains": [],
  "readOnly": true,
  "requiresLogin": false,
  "tags": ["hello", "demo", "smoke-test"]
}
|#

(define (main args)
  (define target
    (if (null? args)
        "world"
        (car args)))
  (string-append "hello " target))
```

把脚本放到本地文件后，你可以直接运行：

```bash
cargo run -- run ./demo.scm -- OpenWalk
```

也可以把它做成工作区 tool，放在：

```text
.openwalk/tools/<tool-name>/main.scm
```

如果脚本没有定义 `main`，OpenWalk 会返回脚本顶层表达式加载后的结果；不过多数情况下，显式写一个 `(main args)` 更清晰，也更利于工具化。

## 环境变量

### 工作区与工具仓库

- `OPENWALK_HOME`
  覆盖全局 home，默认是 `~/.openwalk`
- `OPENWALK_HUB_GIT_URL`
  指定工具 hub 的 git 仓库地址
- `OPENWALK_HUB_GIT_REF`
  指定要拉取的分支、tag 或其他 git ref

例如：

```bash
export OPENWALK_HUB_GIT_URL=https://github.com/weekend-project-space/openwalkhub
export OPENWALK_HUB_GIT_REF=main
```

### 浏览器相关

- `OPENWALK_BROWSER_BIN`
  手动指定 Chromium / Chrome 可执行文件
- `OPENWALK_BROWSER_PROFILE_DIR`
  覆盖非 session 模式下的默认浏览器 profile 目录
- `OPENWALK_NO_SANDBOX`
  启动浏览器时关闭 sandbox
- `OPENWALK_HEADLESS`
  控制非 session 模式下的无头启动
- `OPENWALK_HEADFUL`
  控制非 session 模式下的有头启动

如果你使用的是命名 session，控制有头模式最稳妥的方式是显式写：

```bash
cargo run -- exec browser-open https://example.com -s=qa --headed
```

如果你希望直接在 shell 里运行全局安装的工具，把下面目录加入 `PATH`：

- 默认：`~/.openwalk/bin`
- 自定义 home 时：`$OPENWALK_HOME/bin`

## 当前仓库里的示例工具

- `hello-word`
  最小 smoke test，不依赖浏览器
- `bing-search`
  打开 Bing，执行搜索并返回结构化结果
- `v2ex-hot`
  拉取 V2EX 热门主题并返回结构化结果

可以直接查看它们的实现：

- `.openwalk/tools/hello-word/main.scm`
- `.openwalk/tools/bing-search/main.scm`
- `.openwalk/tools/v2ex-hot/main.scm`

## 注意事项

- `exec` 在目标未命中时会尝试自动安装工具到当前项目，这意味着它可能修改你的 `openwalk.json` 和 `.openwalk/tools/`
- `install` 需要当前项目已经有 `openwalk.json`
- 浏览器能力依赖 Chromium；如果本机没有可探测到的浏览器，相关命令会失败
- 命名 session 会复用独立 profile，适合保存登录态和多账号并行
- 如果你想要当前实现最准确的能力面，请优先使用 `tool list --json` 和 `tool info --json`
