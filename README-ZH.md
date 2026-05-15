# OpenWalk

把网站变成你本地可复用的命令。

OpenWalk 是一个 local-first CLI，把网站能力、真实浏览器自动化和项目工具统一到一个入口里。搜索、热门信息流、页面操作、工作流编排，不再只是一次性脚本，而是可以反复调用、逐步沉淀的命令。

```bash
openwalk exec bing/search "Claude Code" 5
openwalk exec reddit/hot LocalLLaMA
openwalk exec v2ex/hot
openwalk exec openwalkhub/tools

openwalk exec browser-open https://news.ycombinator.com -s=demo
openwalk exec page-snapshot -s=demo
```

你拿到的不是一堆零散文本，而是更适合继续处理的结构化结果：

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

如果某个工具还没安装，`openwalk exec` 可以先从 hub 拉取到当前项目，再立即执行。

## 为什么会让人想继续用

- 把网站能力变成命令，而不是一次性脚本
- 用真实浏览器完成自动化，而不是只停留在离线抓取
- local-first，更容易复用你的登录态、文件和本地环境
- 对人友好，也对 AI Agent 友好
- 现成工具可以直接跑，自定义工具可以慢慢沉淀成团队资产

## 30 秒上手

### 1. 安装

```bash
# Linux
curl -fsSL https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.sh | bash
```

```powershell
# Windows PowerShell
iex "& { $(irm https://raw.githubusercontent.com/weekend-project-space/openwalk/main/scripts/install.ps1) }"
```

### 2. 初始化当前项目

```bash
openwalk init
```

### 3. 跑第一条命令

```bash
openwalk exec bing/search "OpenAI" 5
openwalk exec reddit/hot LocalLLaMA
openwalk exec v2ex/hot
```

### 4. 打开一个真实浏览器会话

```bash
openwalk exec browser-open https://example.com -s=demo
openwalk exec page-snapshot -s=demo
openwalk exec browser-close -s=demo
```

## 一个入口，统一三类能力

### 现成的网站命令

把常见网页操作直接变成稳定命令：

- `bing/search`
- `reddit/hot`
- `v2ex/hot`
- 以及更多来自 hub 的 `search`、`hot`、`detail`、`comments`、`download` 一类工具

这类命令适合直接给人用，也适合接到脚本、流水线和 Agent 工作流里。

### 内建的浏览器原语

OpenWalk 内建一组可以直接调用的浏览器能力：

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

你可以直接使用这些能力，也可以用它们逐步拼出自己的网页工作流。

### 项目级和全局级工具

OpenWalk 不只是跑现成命令，也支持把能力长期沉淀下来：

- 工作区工具
- 全局工具
- 从 hub 拉取的工具
- 本地 `.scm` 脚本

临时脚本不需要一直停留在“临时”阶段。它可以慢慢长成团队真正会复用的工具。

## openwalkhub 当前支持的功能

OpenWalk 可以在执行时从 `openwalkhub` 拉取工具到当前项目。下面这份表更新日期 `2026-05-13` 。

| 站点 / 分类     | 当前命令                                                                                            |
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

其中比较适合拿来直接展示给新用户的命令包括：

- 热门与时间线：`reddit/hot` `v2ex/hot` `linuxdo/hot` `weibo/hot` `zhihu/hot` `hackernews/top` `producthunt/today`
- 搜索：`bing/search` `reddit/search` `twitter/search` `youtube/search` `zhihu/search` `stackoverflow/search`
- 详情与内容读取：`v2ex/topic` `zhihu/question` `hackernews/thread` `reddit/thread` `youtube/video` `youtube/transcript`
- 平台与账号数据：`github/me` `github/issues` `twitter/bookmarks` `twitter/notifications` `linkedin/profile`

## 给人用，也给 AI Agent 用

### 对人

当你想把重复网页操作变成稳定命令时，OpenWalk 很顺手：

- `openwalk exec openwalkhub/tools` 看看hub上有什么工具
- `openwalk tool list` 看看现在能跑什么
- `openwalk tool info bing/search` 查看某个工具怎么用
- 直接执行现成网站命令，而不是每次重新打开网页手动点一遍

### 对 AI Agent

如果你在给 AI Agent 找一个稳定的执行面，OpenWalk 很自然：

- Agent 可以调用现成命令，而不是每次从零探索网站
- 浏览器能力可以复用真实本地环境
- 结构化输出更容易接入后续推理、提取和编排
- 已经探索过一次的网站流程，可以沉淀成下一次直接可跑的命令

## 从这里继续

```bash
openwalk exec openwalkhub/tools
openwalk tool list
openwalk tool info bing/search
openwalk tool info browser-open
```

完整手册、安装细节、工具编写方式、环境变量和目录结构见 [GUIDE.md](./GUIDE.md)。
