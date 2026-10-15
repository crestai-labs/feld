# Quickstart

<a href="quickstart.md">English</a> · 简体中文 · <a href="quickstart.ja.md">日本語</a>

安装 Feld，并从 Feld 操作 tmux 上的 Claude Code。大约需要 10 分钟。

本指南以 Claude Code 和 tmux 为前提。Codex 和 herdr 的不同之处，会在相关位置注明。各节命令的选项和输出见 [CLI 参考](cli.md)，其他面向用户的文档见 [Guide](README.md)，contract 的正本见 [`docs/spec/`](../spec/)。

1. [前提条件](#前提条件)
2. [安装](#安装)
3. [初始设置](#初始设置)
4. [attach 手动启动的 Claude Code](#attach-手动启动的-claude-code)
5. [从 Feld 启动 Claude Code](#从-feld-启动-claude-code)
6. [发送 Message](#发送-message)
7. [创建并运行 Job](#创建并运行-job)
8. [Web UI](#web-ui)
9. [清理](#清理)
10. [卸载](#卸载)

## 前提条件

- tmux 3.2 及以上（herdr 0.9.0 及以上）
- Claude Code 2.1.181 及以上（Codex 0.156.0 及以上），已安装并完成认证

## 安装

使用 Homebrew（macOS，或装有 Homebrew 的 Linux）：

```sh
brew install crestai-labs/tap/feld
```

否则使用安装脚本。脚本从 GitHub Release 下载适合本机的 archive，校验 checksum，并把 `feld` 和随附的 Plugin 放到 `~/.local/bin`。可以用 `FELD_INSTALL_DIR` 选择位置，用 `FELD_VERSION` 选择版本。

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
```

如果 PATH 中没有 `~/.local/bin`，请加上。从源码构建的方法见 [CONTRIBUTING.md](../../CONTRIBUTING.md#development-setup)。

## 初始设置

在终端运行 `feld init` 并回答问题。它会创建 `FELD_HOME`（默认 `~/.feld`），把从随附 Plugin 中选出的那些放到 `~/.feld/plugins/`，写入 Claude Code 和 shell 的设置，并启动 daemon。`--yes` 供脚本使用，不询问即同意全部。第一次请在终端作答，并确认它改写了什么。

```sh
feld init
```

1. **选择 Plugin**：用 `↑` `↓` 在项目间移动，用 `Space` 切换勾选，用 `Enter` 确认。PATH 上找到的 Runtime（`claude`）和 Provider（`tmux`）的 Plugin 一开始就已勾选
2. **Claude Code 设置**：`claude-code` Runtime Adapter 把 Feld 的 skill 和 Claude Code plugin 写入 Claude Code config root（`CLAUDE_CONFIG_DIR`，未设置时为 `~/.claude`）下的 `skills/feld/`，并把同一 root 下 `settings.json` 中的 `otelHeadersHelper` 和 `statusLine` 指向它。对这两项，它会显示当前值和将要写入的值，并征求同意。如果你有自己的 `statusLine`，其命令会保留在 Feld status line helper 的 `--chain` 中，status line 会在 Feld 的区块旁边显示你的输出。你自己的 `otelHeadersHelper` 会被替换，原值保存在 `~/.feld/prepared/claude-code.json`
3. **shell 设置**：在 shell 配置文件（zsh 为 `~/.zshrc`，bash 为 `~/.bashrc`）末尾追加 `eval "$(feld env)"`。从加载了这一行的 shell 启动的 Claude Code，会把 telemetry 发送到 daemon 的 Collector
4. **启动 daemon**：daemon 启动 Control API 和 Runtime API、Web UI 以及 OTel Collector，并加载 `~/.feld/plugins/` 中的 Plugin

完成后，`next:` 显示要依次运行的命令，`ui:` 显示 Web UI 的 URL，`note:` 显示针对各 Runtime 的提示。按照 `next:`，`source` 配置文件或打开新的 shell。

用下面的命令查看 daemon 的状态。`runtimes:` 和 `providers:` 显示已加载的 Plugin。已停止的 daemon 用 `feld daemon start` 启动。要检查环境，请运行 `feld doctor`，它会把每项检查的结果和修复方法各显示为一行。

```sh
feld daemon status
```

> [!NOTE]
> **Codex**：在列表中也勾选 `codex`。`codex` Runtime Adapter 把 skill 和 hook 声明写入 config root（`CODEX_HOME`，未设置时为 `~/.codex`），并把 Feld home 的 `spool/shared` 加入 `config.toml` 的 `writable_roots`。下次启动 Codex 时，它会询问是否信任 Feld 的 hook，在信任了它们的 Codex 中集成即启用。认证使用 Codex 自己的登录。sandbox 和 spool 的细节见 [`spec/design/adapter/codex.md`](../spec/design/adapter/codex.md)
>
> **herdr**：在 herdr server 运行的状态下，勾选列表中的 `herdr`。herdr API 只能连到正在运行的 server，所以使用 Feld 期间请保持 server 运行。后续命令相同，Session 会放在 herdr 的 workspace 中。用 `--herdr-session` / `--herdr-unit` / `--herdr-workspace` / `--herdr-pane` / `--herdr-split` 指定放到哪个 server 的哪个 unit
>
> **从脚本放置**：用 `--runtime` / `--provider` 传入答案。要放置两个同类的，就把该 flag 传两次。更改了 Plugin 的 `feld init` 会询问是否重启正在运行的 daemon

CLI 参考：[`feld init`](cli.md#feld-init)、[`feld daemon`](cli.md#daemon)、[`feld doctor`](cli.md#feld-doctor)、[launchSpec 选项](cli.md#launchspec-options)

## attach 手动启动的 Claude Code

从加载了 `eval "$(feld env)"` 的 shell 打开 tmux，并在 pane 中启动 Claude Code。

```sh
tmux new-session -s feld-claude
claude
```

请 Claude Code 运行下面的命令。在 Claude Code 中运行 `/feld attach` 会以自动生成的名字 attach，`/feld attach <ACTOR>` 则以 `<ACTOR>` 的名字 attach。

```sh
feld actor attach manual-claude --self --runtime claude-code
```

Feld 把正在运行的 Claude Code 记录为 `manual-claude` Actor 的已 attach Session。Claude Code 的启动和退出仍由用户完成。skill、hook 和 Plugin Monitor 对 `feld init` 之后启动的 Claude Code 生效（已经打开的 Claude Code 在 `/reload-plugins` 之后生效），telemetry 对 `feld init` 之后启动的 Claude Code 生效。没有做这些准备的 Claude Code 也可以 attach，输出中的 `note:` 行会指引你运行 `feld init`。对于从没有 `feld env` 的 shell 启动的 Claude Code，attach 输出中的 `telemetry` 行会把 metrics 显示为 `unavailable` 并给出原因。

从另一个终端查看 attach 的结果。

```sh
feld actor get manual-claude
```

> [!NOTE]
> **Codex**：请 Codex 运行 `feld actor attach manual-codex --self --runtime codex`。attach 是 Admin 操作，所以在 sandbox 之外运行。sandbox 内的 `feld` 会被拒绝，Codex 会请求批准在 sandbox 外运行；批准后 attach 即可通过。对于不请求批准的 Codex（`approval_policy` 为 `never`），请在 sandbox 外的 shell 中运行拒绝信息附带的命令，它指向显示 Codex 的 pane。已 attach Session 的 metrics 为 `unavailable`，hook 发送的 context window 使用率除外

CLI 参考：[`feld actor attach`](cli.md#feld-actor-attach)

## 从 Feld 启动 Claude Code

创建 `claude-dev` Agent。创建时，Agent 会在 tmux 上启动 Claude Code。

```sh
feld actor create claude-dev --kind agent --runtime claude-code --workdir "$PWD"
```

对于启动的 Claude Code，`claude-code` Runtime Adapter 会准备每个 Session 的设置和 Claude Code plugin，并从启动起就传入 skill、hook、Plugin Monitor 和 telemetry 的设置。用下面的命令读取 Session 的对话。

```sh
feld session log claude-dev
```

用下面的命令请求结束由 Feld 启动的 Session。

```sh
feld session stop claude-dev
```

Feld 向 Claude Code 的 pane 发送 `/exit`，若有任务在运行则再发送对退出确认的回答，Session 随之结束。`feld actor start claude-dev` 启动下一个 Session，`feld actor restart claude-dev` 停止当前 Session 并换成下一个。要结束已 attach 的 Session，请退出所 attach 的 Claude Code 本身。

> [!NOTE]
> **Codex**：用 `--runtime codex` 创建。model 为 Codex 的默认值；model 和 approval policy 等 Codex 自身的设置，作为 `config.toml` 的条目传给 `--runtime-settings`。`<available-model>` 使用启动 Codex 后 `/model` 列出的名字。收到结束请求时，Feld 向 Codex 的 pane 发送 `/quit`
>
> ```sh
> feld actor create codex-dev --kind agent --runtime codex --workdir "$PWD"
> feld actor update codex-dev --runtime-settings '{"model":"<available-model>","approval_policy":"never"}'
> feld actor restart codex-dev
> ```
>
> 在 `config.toml` 中标记为 trusted 的 workdir 里，Codex 会立即等待输入；在其他 workdir 里，它会显示信任确认并等待。对于允许 workdir 的 `.codex/` 设置和 hook 无需确认即生效的 repo，用 `--trust-workdir` 声明
>
> ```sh
> feld actor create codex-dev --kind agent --runtime codex --workdir "$PWD" --trust-workdir
> ```

CLI 参考：[`feld actor create`](cli.md#feld-actor-create)、[`feld session log`](cli.md#session)、[`feld session stop`](cli.md#feld-session-stop)、[launchSpec 选项](cli.md#launchspec-options)

## 发送 Message

从另一个终端向 Claude Code 发送 Message。收件人可以是已 attach 的 `manual-claude`，也可以是 Feld 启动的 `claude-dev`。

```sh
feld message send claude-dev "Review the structure of this repository"
```

在启用了 Plugin Monitor 的 Claude Code 中，Message 会自动投递到 Session。在尚未启用 Plugin Monitor 的 Session 中，例如 `feld init` 之前就已打开的 Claude Code，请 Claude Code 运行下面的命令来接收 Message。

```sh
feld self message recv
```

从外部终端查看 Actor 的 mailbox。从外部终端发送的 Message，其 `from` 为 `Admin`。

```sh
feld message list --to claude-dev
```

> [!NOTE]
> **Codex**：queue helper 把 Message 放入 Codex 的队列，它会成为下一个 turn 的输入

CLI 参考：[`feld message`](cli.md#message)、[`feld self`](cli.md#self)

## 创建并运行 Job

创建 `hello` Job 并立即启动。

```sh
feld actor create hello --kind job --start -- sh -c 'printf "Hello from Feld\n"'
```

`--` 之后的全部内容成为 Job 的命令。只有加上 `--start`，Job 才会在创建时启动 Session。Job 的定义和 Session 记录在 `default` Workspace 中。

查看 Actor 列表、Job 的详情，以及 Feld 留下的 record。

```sh
feld actor list -a
feld actor get hello
feld record list --actor hello
```

Session ID 列在 `feld actor get` 的 `Sessions` 下。传入 Session ID 读取 Job 的输出。

```sh
feld session log <session>
```

要用同一定义启动新的 Session，运行下面的命令。

```sh
feld actor start hello
```

`feld session get` 中的 `spawned_by Admin` 和 `feld record list` 中的 `by Admin` 指的是来自这个终端的操作。使用 daemon 本地 token 的 CLI 操作记录为 [Admin](../spec/concepts/glossary.md#admin)。Admin 是 Actor 之外的 principal，只有拥有 mailbox 的 Actor 才能作为 Message 的收件人。

加上 `--cron`，它就成为带 Schedule 的 Job；`feld actor pause` / `feld actor resume` 停止和恢复 Schedule。

```sh
feld actor create nightly --kind job --cron '0 9 * * *' --timezone Asia/Tokyo -- ./nightly.sh
```

CLI 参考：[`feld actor`](cli.md#actor)、[`feld session`](cli.md#session)、[`feld record`](cli.md#record)

## Web UI

Sign in 并在默认浏览器中打开 Web UI。

```sh
feld ui
```

要只打印 URL，运行下面的命令。

```sh
feld ui --print-url
```

URL 的 fragment 包含一次性的 Sign in code。请在 5 分钟内用该浏览器打开。

daemon 只响应发送到 loopback 的请求，以及发送到用 `--allow-host` 允许的名字的请求。要从另一台设备的浏览器打开，请通过反向代理转发 daemon 的 Port，并在启动 daemon 时允许该 host 名（`feld daemon restart --allow-host <host>`）。在该浏览器打开的 Web UI 的 Sign in 页面上，输入 `feld ui pair` 在 daemon 所在机器上打印的 code。

```sh
feld ui pair --name laptop
```

> [!NOTE]
> 反向代理以 Tailscale 为前提。用 `tailscale serve --bg <PORT>` 转发 daemon 的 Port，并用 `--allow-host` 允许 `tailscale serve status` 显示的名字（`<host>.<tailnet>.ts.net`）。

用 `feld browser list` 列出已 Sign in 的浏览器，用 `feld browser revoke laptop` 让其中一个 Sign out。Settings 中的 Sign out 会让该浏览器自身 Sign out。

各 view 绘制的内容见 [Web UI 视图](webui.md)。

CLI 参考：[`feld ui`](cli.md#feld-ui)

## 清理

删除用完的 Actor。历史会保留，同一名字可以立即重用。当前 Session 仍在运行的 Actor，请先用 `feld session stop` 停止。

```sh
feld actor delete hello
```

停止 daemon。

```sh
feld daemon stop
```

CLI 参考：[`feld actor delete`](cli.md#feld-actor-delete)、[`feld daemon stop`](cli.md#feld-daemon-stop)

## 卸载

要移除 Feld，运行 `feld uninstall`。它会停止 daemon 和由 Feld 启动的 Session，还原 `feld init` 写入 Claude Code 和 Codex config root 的内容，并从 Feld home 移除设置。它会在询问之前列出将要移除的内容。

```sh
feld uninstall
```

`sessions/` 和 store 会保留，因此再次运行 `feld init` 就能读到同样的历史。要把它们也移除，加上 `--purge`。最后，按照 `next:` 的各行，从 shell 配置文件中移除 `eval "$(feld env)"`，并删除 `feld` 可执行文件（通过 Homebrew 安装的话，运行 `brew uninstall feld`）。

CLI 参考：[`feld uninstall`](cli.md#feld-uninstall)
