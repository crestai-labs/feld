<p align="center">
  <img src="docs/assets/feld-logo-light.png#gh-light-mode-only" alt="Feld" width="160" />
  <img src="docs/assets/feld-logo-dark.png#gh-dark-mode-only" alt="Feld" width="160" />
</p>

<h1 align="center">Feld</h1>

<p align="center"><strong>Agent Loop Control Plane</strong></p>

<p align="center">
  <a href="https://github.com/crestai-labs/feld/actions/workflows/ci.yml"><img src="https://github.com/crestai-labs/feld/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI" /></a>
  <a href="https://github.com/crestai-labs/feld/releases/latest"><img src="https://img.shields.io/github/v/release/crestai-labs/feld?sort=semver" alt="Release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License: Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey" alt="Platform: macOS | Linux" />
</p>

<p align="center">
  <a href="#支持的产品">支持的产品</a> · <a href="#安装">安装</a> · <a href="#开始使用">开始使用</a> · <a href="docs/guide/quickstart.md">Quickstart</a> · <a href="#文档">文档</a>
</p>

<p align="center">
  <a href="README.md">English</a> · 简体中文 · <a href="README.ja.md">日本語</a>
</p>

<p align="center">
  <img src="docs/assets/readme/actors-light.png#gh-light-mode-only" alt="Feld 的 Web UI，按 Actor 排列 Session 的时间线和 metrics" width="960" />
  <img src="docs/assets/readme/actors-dark.png#gh-dark-mode-only" alt="Feld 的 Web UI，按 Actor 排列 Session 的时间线和 metrics" width="960" />
</p>

---

Feld 是面向以循环方式运行多个 Coding Agent 的开发者的、与 Harness 无关的 **Agent Loop Control Plane**。它让每个 Session 保持可见，支持 Agent 之间的协作和人的控制。

> [!NOTE]
> Feld 仍处于实验阶段。在 0.x 期间，CLI、API 和 store 的形式可能发生不兼容的变化（[兼容性承诺](docs/guide/compatibility.md)）。

## Why Feld?

把多个 Agent 和定时任务组合成循环或 Harness（或者 Software Factory）时，常常会在相同的地方遇到问题：

- 已经停止的 Agent，看起来仍在运行。
- cron 或 launchd 的定时任务，不知何时悄无声息地停了。
- 上一次运行还没结束，下一次就启动了，同样的工作开始了两遍。
- 发给已停止 Agent 的请求没有送达，就此消失。
- Claude Code 和 Codex 的启动方式和状态的呈现方式都不一样。

这些问题的根源都在于：从 tmux、cron 或自制消息机制的状态去推测"谁在运行""什么时候发生了什么"。

Feld 接管这一部分。它为 Agent 和 Job 赋予持久的 identity，把 Session 的启动与结束、Message 的投递、定时任务的触发与跳过，作为事实记录在一条持久的时间线上。发给已停止 Agent 的 Message，由它的下一个 Session 读取。Claude Code 和 Codex 使用同一套 identity 和 Message 机制。人可以通过 Web UI 和 CLI 看到什么正在运行、哪里需要处理，并决定下一步。

接下来运行哪个 Agent、何时放弃等策略，留在使用者的 Harness 中。Harness 可以自由选择，Feld 在其下方充当通用的 control plane。

## Feld 不是什么

- **不是 Coding Agent。** Feld 本身不写代码，也不取代 Claude Code 或 Codex。工作由 Coding Agent 完成，Feld 提供跨已登记 Session 的持久 identity、历史和 Message。
- **不是 Harness、Agent framework 或 LLM SDK。** Feld 不决定 model、prompt、tool 和 plan，也不决定下一个运行哪个 Agent。使用者带来 Harness 和 Runtime，Feld 在其周围提供通用的 control plane。
- **不是 log viewer。** log 只是 Feld 处理的信息的一部分。Feld 还把 lifecycle record、Message、metrics 以及对 Feld-owned Session 的控制整合在一起。

## 工作原理

<p align="center">
  <img src="docs/assets/readme/how-it-works-zh-CN-light.png#gh-light-mode-only" alt="Feld 的工作原理：CLI 和 Web UI 使用 Feld daemon，daemon 启动和停止 tmux 或 herdr 中的 Coding Agent（Claude Code / Codex），投递 Message，并接收 transcript 和 metrics" width="360" />
  <img src="docs/assets/readme/how-it-works-zh-CN-dark.png#gh-dark-mode-only" alt="Feld 的工作原理：CLI 和 Web UI 使用 Feld daemon，daemon 启动和停止 tmux 或 herdr 中的 Coding Agent（Claude Code / Codex），投递 Message，并接收 transcript 和 metrics" width="360" />
</p>

处于中心的是 Feld daemon。人和 Harness 通过 CLI 和 Web UI 使用 daemon。Coding Agent 在 tmux 或 herdr 中照常运行。daemon 启动和停止它们的 Session，投递 Message，并把从 Agent 收到的 transcript 和 metrics 记录在一条时间线上。

| 概念 | 含义 |
|---|---|
| Actor | 有名字的持久主体，分为两种：<ul><li>Agent（Claude Code / Codex）</li><li>Job（任意命令，也可以用 cron 定时运行）</li></ul> |
| Session | Actor 的一次运行，分为两种：<ul><li>由 Feld 启动的</li><li>附加（attach）到已在运行的 Agent 上的</li></ul> |
| Message | Actor 之间、人与 Actor 之间往来的请求或报告。发给已停止 Actor 的 Message 由下一个 Session 读取 |

完整的术语及其关系见[术语表](docs/spec/concepts/glossary.md)。

## 支持的产品

| 类别 | 产品 | 支持的版本 |
|---|---|---|
| OS | macOS（Apple Silicon） | — |
| | Linux（x86_64） | glibc 2.35 及以上 |
| 执行环境 | tmux | 3.2 及以上 |
| | [herdr](https://herdr.dev) | 0.9.0 及以上 |
| Coding Agent | Claude Code | 2.1.181 及以上 |
| | Codex | 0.156.0 及以上 |

执行环境和 Coding Agent 各装有至少一个即可运行。请事先完成 Coding Agent 的安装和认证。

## 安装

使用 Homebrew（macOS，或装有 Homebrew 的 Linux）：

```sh
brew install crestai-labs/tap/feld
```

否则使用安装脚本：

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
```

脚本从 GitHub Release 下载适合本机的 archive，校验 checksum，并把 `feld` 和随附的 Plugin 放到 `~/.local/bin`。可以用 `FELD_INSTALL_DIR` 更改位置，用 `FELD_VERSION` 指定版本。从源码构建的方法见 [CONTRIBUTING.md](CONTRIBUTING.md#development-setup)。

## 升级

```sh
brew upgrade feld   # 或再次运行安装脚本
feld init
```

`brew upgrade feld` 和安装脚本都会替换可执行文件。再次运行 `feld init` 会把新的 Plugin 放到 `~/.feld/plugins/`，替换 Claude Code 和 Codex 设置中的 Feld skill 与 hook，并重启 daemon。daemon 启动时会自动迁移 store，Session 在重启期间继续运行，因此无需停止 Agent 即可升级。详见 [兼容性](docs/guide/compatibility.md#upgrading)。

## 开始使用

```sh
feld init
feld ui
```

用 `feld init` 进行初始设置，用 `feld ui` 在浏览器中打开 Web UI。

例如，要把 Claude Code 连接到 Feld，请从新的 shell 在 tmux 或 herdr 的 pane 中启动 `claude`，然后运行：

```text
/feld attach
# 或者请求"把你自己 attach 到 Feld"
```

后续内容见 [Quickstart](docs/guide/quickstart.md)。

## 用法

```sh
# 从 Feld 启动 Agent（Codex 使用 --runtime codex）
feld actor create developer-01 --kind agent --runtime claude-code --workdir "$PWD"

# 发送 Message
feld message send developer-01 "完成修改，请 reviewer-01 审查，并报告结果"

# 创建按 cron 运行的 Job
feld actor create dispatcher --kind job --cron "0 * * * *" -- python3 dispatch.py

# 查看状态和历史
feld actor list
feld session log developer-01
feld record list --actor dispatcher
```

所有命令和选项见 [CLI 参考](docs/guide/cli.md)。卸载时，请运行 `feld uninstall`。

## FAQ

**支持哪些操作系统？**
macOS（Apple Silicon）和 Linux（x86_64）。详见[支持的产品](#支持的产品)。

**可以在 Windows 上使用吗？**
可以在 WSL2 中使用。原生支持正在考虑中。

**有 Docker image 吗？**
没有。daemon 在 host 上运行。

**会向外部发送数据吗？**
不会，也不发送使用统计。Feld 读取和写入的内容见 [Data and privacy](docs/guide/data-and-privacy.md)。

**它会取代 Claude Code 或 Codex 吗？**
不会。Feld 让它们照常运行，并在周围加上 control plane。

## 文档

| 文档 | 内容 |
|---|---|
| [Quickstart](docs/guide/quickstart.md) | 首先阅读的入门指南 |
| [CLI 参考](docs/guide/cli.md) | 命令、选项和输出 |
| [兼容性](docs/guide/compatibility.md) | 升级方法，以及 0.x 期间的兼容性承诺 |
| [Data and privacy](docs/guide/data-and-privacy.md) | Feld 不发送什么、读取和写入什么、如何删除 |
| [`docs/spec/`](docs/spec/) | 规范的正本（术语表、CLI、daemon API、Web UI、设计） |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 从源码构建、测试、提交 Issue 和 PR 的方法 |

## License

Feld 以 [Apache License 2.0](LICENSE) 提供。可以自由使用（包括商业用途），也可以修改和再分发。选择 Apache-2.0 是因为它明确授予专利许可，企业也可以放心采用。

由 Kazuki Yoshida 创建，CrestAI 负责开发和维护。

著作权声明见 [NOTICE](NOTICE)，商标的使用见 [TRADEMARK.md](TRADEMARK.md)。

分发的 archive 附有所含第三方软件及其许可证的列表（`THIRD-PARTY-LICENSES.md`）。
