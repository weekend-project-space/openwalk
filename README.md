# OpenWalk

OpenWalk 是一个 local-first 的 Scheme runtime CLI，用来执行本地 Scheme tool，并提供浏览器自动化 host function。

```bash
openwalk <command>
```

## 安装

推荐直接安装 release 二进制：

```bash
# Linux
curl -fsSL https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.sh | bash
```

```powershell
# Windows PowerShell
irm https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.ps1 | iex
```

脚本默认会：

- 下载 GitHub Releases 中当前平台对应的 `openwalk` 二进制
- 安装到 `~/.openwalk/bin` 或 `%USERPROFILE%\.openwalk\bin`
- 尝试把该目录加入当前用户的 `PATH`

注意：

- 安装脚本依赖已发布的 GitHub Release 资产
- 如果看到 `releases/latest/download/... 404`，通常说明仓库还没有发布 release，或者缺少对应平台文件

常用参数：

```bash
bash scripts/install.sh --version v0.1.0
bash scripts/install.sh --install-dir /usr/local/bin --no-path
```

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -Version v0.1.0
powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -InstallDir C:\tools\openwalk -NoPath
```

## 快速开始

```bash
# 初始化当前项目
openwalk init

# 查看可用工具
openwalk  tool list

# 运行一个工作区 tool
openwalk exec hello-word OpenWalk

# 打开浏览器并使用命名会话
openwalk exec browser-open https://example.com -s=demo
openwalk exec tab-list -s=demo
openwalk exec -close -s=demo
```

## 核心规则

- `run` 只运行本地 `.scm` 文件或工作区 tool
- `exec` 可以运行内建 host function、本地 `.scm`、工作区 tool、全局 tool
- `exec` 未命中时，会尝试从 hub 拉取工具到当前项目后再执行
- 输出格式支持 `yaml`、`md`、`json`
- `tool list` 默认输出紧凑工具列表，`tool info` 默认输出可读说明页
- 浏览器状态默认持久化

## 核心命令

| 命令             | 用法                                   | 说明                                                 |
| ---------------- | -------------------------------------- | ---------------------------------------------------- |
| `init`           | `openwalk init`                        | 初始化当前项目，创建 `openwalk.json` 和 `.openwalk/` |
| `install`        | `openwalk install`                     | 安装 `openwalk.json -> tools` 中声明的项目工具       |
| `run`            | `openwalk run <tool-or-script>`        | 运行工作区 tool 或本地 `.scm` 文件                   |
| `exec`           | `openwalk exec <tool-or-script>`       | 执行 host function、脚本、工作区 tool、全局 tool     |
| `tool list`      | `openwalk tool list [-f <fmt>]`        | 查看可直接执行的工具列表                             |
| `tool info`      | `openwalk tool info <tool> [-f <fmt>]` | 查看工具用法与元信息说明页                           |
| `tool add`       | `openwalk tool add <package>`          | 安装工具到当前项目                                   |
| `tool remove`    | `openwalk tool remove <package>`       | 从当前项目移除工具                                   |
| `tool install`   | `openwalk tool install <package>`      | 全局安装工具                                         |
| `tool uninstall` | `openwalk tool uninstall <package>`    | 全局卸载工具                                         |

## `run` 与 `exec`

| 命令                     | 目标                      | 解析顺序                                                                            |
| ------------------------ | ------------------------- | ----------------------------------------------------------------------------------- |
| `openwalk run <target>`  | 本地脚本、工作区 tool     | 本地 `.scm` -> 工作区 tool                                                          |
| `openwalk exec <target>` | host function、脚本、工具 | 本地 `.scm` -> 工作区 tool -> 内建 host function -> 全局 tool -> 自动拉取到当前项目 |

例子：

```bash
# 工作区 tool
cargo run -- run hello-word -- OpenWalk

# 本地脚本
cargo run -- run ./demo.scm -- foo bar

# 内建浏览器命令
cargo run -- exec browser-open https://example.com
```

注意：`browser-open` 这类内建命令不能用 `run`。

## 常用运行参数

| 参数                             | 说明                                             |
| -------------------------------- | ------------------------------------------------ |
| `-s <name>` / `--session <name>` | 指定浏览器会话                                   |
| `-f <fmt>` / `--format <fmt>`    | 输出格式：`yaml`、`md`、`json`                   |
| `--`                             | 停止解析 OpenWalk 运行参数，后续参数原样传给脚本 |

例子：

```bash
cargo run -- run hello-word -f=json -- OpenWalk
cargo run -- exec browser-open https://example.com -s=qa
cargo run -- run ./demo.scm -- -f=json
```

## Tool 列表与详情

```bash
# 默认输出紧凑列表
cargo run -- tool list

# 返回结构化对象数组
cargo run -- tool list -f=json

# 查看内建 tool 的说明页
cargo run -- tool info browser-open

# 查看工作区 tool 的说明页
cargo run -- tool info hello-word
```

- `tool list` 默认会列出可直接执行的 builtin、workspace、global tools。
- `tool list -f=json` 返回精简结构：`name`、`usage`、`description`、`source`。
- `tool info` 默认会展示 `usage`、`args`、`options`、`returns`、`examples` 等信息。
- `tool info` 支持 builtin 名称、已安装 tool ref，以及本地 `.scm` 脚本路径。

## 常用浏览器命令

| 命令              | 用法                                   | 说明             |
| ----------------- | -------------------------------------- | ---------------- |
| `browser-open`    | `openwalk exec browser-open <url>`     | 打开浏览器并导航 |
| `page-goto`       | `openwalk exec page-goto <url>`        | 当前页面跳转     |
| `page-screenshot` | `openwalk exec page-screenshot <path>` | 页面截图         |
| `tab-list`        | `openwalk exec tab-list`               | 列出标签页       |
| `tab-new`         | `openwalk exec tab-new [url]`          | 新建标签页       |
| `tab-select`      | `openwalk exec tab-select <tab>`       | 切换标签页       |
| `tab-close`       | `openwalk exec tab-close [tab]`        | 关闭标签页       |
| `browser-close`   | `openwalk exec browser-close`          | 关闭浏览器会话   |

快速浏览能力面：

```bash
cargo run -- tool list
```

结构化能力面：

```bash
cargo run -- tool list -f=json
```

## 浏览器会话

- 默认 profile：`~/.openwalk/browser-profile/default`
- 命名会话 profile：`~/.openwalk/browser-profile/<session>`
- `tab-list`、`tab-new`、`tab-select`、`tab-close` 需要先有已打开的浏览器会话

推荐顺序：

```bash
cargo run -- exec browser-open https://example.com -s=qa
cargo run -- exec tab-list -s=qa
cargo run -- exec browser-close -s=qa
```

`browser-open` 额外支持：

```bash
cargo run -- exec browser-open https://example.com --headed
cargo run -- exec browser-open https://example.com --new-tab -s=qa
cargo run -- exec browser-open https://example.com --profile /tmp/openwalk-profile
```

## 工作区与全局目录

项目级：

- `openwalk.json`
- `.openwalk/tools/<package>/main.scm`

全局默认目录：`~/.openwalk`

- `~/.openwalk/repo/tools/<package>/main.scm`
- `~/.openwalk/bin/<package>`
- `~/.openwalk/browser-profile/default`

## Scheme Tool

OpenWalk 通过 `scheme4r` 执行脚本，并注入：

- `openwalk-args`
- `openwalk-script-path`

推荐脚本形状：

```scheme
(define (main args)
  ...)
```

最小示例：

```scheme
(define (main args)
  (if (null? args)
      "hello world"
      (string-append "hello " (car args))))
```

本地运行：

```bash
cargo run -- run ./demo.scm -- OpenWalk
```

工作区 tool 路径：

```text
.openwalk/tools/<tool-name>/main.scm
```

## 环境变量

| 变量                           | 说明                              |
| ------------------------------ | --------------------------------- |
| `OPENWALK_HOME`                | 覆盖全局 home，默认 `~/.openwalk` |
| `OPENWALK_HUB_GIT_URL`         | 指定工具 hub 仓库                 |
| `OPENWALK_HUB_GIT_REF`         | 指定工具 hub 分支或 ref           |
| `OPENWALK_BROWSER_BIN`         | 指定 Chromium / Chrome 可执行文件 |
| `OPENWALK_BROWSER_PROFILE_DIR` | 覆盖默认非 session profile        |
| `OPENWALK_NO_SANDBOX`          | 启动浏览器时关闭 sandbox          |
| `OPENWALK_HEADLESS`            | 控制非 session 模式无头启动       |
| `OPENWALK_HEADFUL`             | 控制非 session 模式有头启动       |
