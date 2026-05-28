# OpenWalk

把网站变成你本地可复用的命令。

OpenWalk 是一个 local-first CLI，用一个入口统一运行网站工具、内建浏览器自动化能力和本地 Scheme 工具：

```bash
openwalk exec <tool> [args...]
```

当一个网页流程不该再停留在一次性脚本里，而应该被反复运行、查看、复用，甚至交给 Agent 调用时，就可以把它放进 OpenWalk。

```bash
openwalk exec openwalkhub/tools
openwalk tool ls

openwalk exec bing/search "Claude Code" 5
openwalk exec reddit/hot LocalLLaMA
openwalk exec v2ex/hot

openwalk exec browser-open https://news.ycombinator.com -s=demo
openwalk exec page-snapshot -s=demo
openwalk exec browser-close -s=demo
```

如果某个 hub 工具还没有安装，`openwalk exec` 可以把它拉取到当前项目并立即执行。

## 为什么用 OpenWalk

- 把网站能力变成命令，而不是一次性脚本
- 需要真实浏览器时，不必停在离线抓取
- 复用本地登录态、浏览器 profile、文件和环境
- 给人和 AI Agent 提供同一个稳定执行面
- 输出保持结构化，方便查看、处理和接到后续流程里

## 安装

```bash
# Linux
curl -fsSL https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.sh | bash
```

```powershell
# Windows PowerShell
iex "& { $(irm https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.ps1) }"
```

## 快速开始

查看本地当前可直接运行的工具：

```bash
openwalk tool ls
```

发现 hub 上的工具：

```bash
openwalk exec openwalkhub/tools
```

运行网站命令：

```bash
openwalk exec bing/search "OpenAI" 5
openwalk exec reddit/hot LocalLLaMA
openwalk exec v2ex/hot
```

运行前查看工具帮助：

```bash
openwalk exec browser-open --help
openwalk tool info browser-open
openwalk tool info browser-open --format=json
```

`openwalk exec <tool> --help` 和 `openwalk tool info <tool>` 默认展示同一份面向人阅读的帮助信息。需要结构化元数据时，使用 `openwalk tool info <tool> --format=json`。

## 浏览器自动化

打开一个浏览器会话，在这个会话上运行页面命令，然后关闭它：

```bash
openwalk exec browser-open https://example.com -s=demo --headed
openwalk exec page-snapshot -s=demo
openwalk exec browser-close -s=demo
```

`browser-open` 常用参数：

```bash
openwalk exec browser-open https://example.com -s=demo
openwalk exec browser-open https://example.com -s=demo --headed
openwalk exec browser-open https://example.com -s=demo --new-tab
openwalk exec browser-open https://example.com -s=demo --profile /tmp/openwalk-profile
```

默认浏览器模式是 headless。需要看到浏览器界面时，使用 `--headed`。

常用内建浏览器工具包括：

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

## 核心命令

| 命令 | 示例 | 作用 |
| ---- | ---- | ---- |
| `exec` | `openwalk exec browser-open https://example.com` | 运行内建工具、本地脚本、工作区/全局工具或 hub 工具 |
| `install` | `openwalk install` | 安装当前项目声明的工具 |
| `tool ls` | `openwalk tool ls` | 列出当前可直接运行的工具 |
| `tool info` | `openwalk tool info browser-open` | 查看工具用法、参数、选项、返回值和示例 |
| `exec --help` | `openwalk exec browser-open --help` | 不执行工具，只查看工具帮助 |

`exec` 是主入口。它会按这个顺序解析工具：本地脚本、工作区工具、内建 host function、全局工具，最后从 hub 自动安装到当前项目并运行。

## Hub 工具

OpenWalk 可以从 `openwalkhub` 拉取可复用的网站工具。先从这里开始：

```bash
openwalk exec openwalkhub/tools
```

一些常见工具类型包括：

- 搜索：`bing/search`、`reddit/search`、`youtube/search`、`zhihu/search`
- 热门与时间线：`reddit/hot`、`v2ex/hot`、`linuxdo/hot`、`hackernews/top`
- 详情页读取：`v2ex/topic`、`zhihu/question`、`hackernews/thread`、`reddit/thread`
- 平台或账号工作流：`github/issues`、`twitter/bookmarks`、`linkedin/profile`

工具可用性会随时间变化，所以建议用 `openwalk exec openwalkhub/tools` 查看当前列表。

## 给人用，也给 Agent 用

对人来说，OpenWalk 能把重复网页操作变成记得住、跑得动、能继续改进的命令。

对 Agent 来说，OpenWalk 提供了稳定的本地执行面：

- 用 `openwalk exec <tool> --help` 发现工具用法
- 用 `openwalk tool info <tool> --format=json` 获取结构化元数据
- 浏览器命令可以复用真实本地环境
- 结构化输出更适合后续推理、提取和编排

## 从这里继续

```bash
openwalk exec openwalkhub/tools
openwalk tool ls
openwalk exec browser-open --help
openwalk tool info bing/search
```

完整手册、安装细节、工具编写方式、环境变量和目录结构见 [GUIDE.md](./GUIDE.md)。
