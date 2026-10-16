# Quickstart

<a href="quickstart.md">English</a> · 简体中文 · <a href="quickstart.ja.md">日本語</a>

把你正在使用的 Claude Code attach 到 Feld，然后请它启动另一个 Agent，给那个 Agent 发送 Message，并运行一个 Job。大约需要 10 分钟。

本指南以 Claude Code 和 tmux 为前提，Codex 和 herdr 的不同之处见[文末](#codex-和-herdr)。每一步都写明要请 Claude Code 做什么，并点明 Feld，好让它去用 `feld` CLI；其下的 note 解释 Claude Code 运行的命令。同样的命令你也可以自己在终端运行。各命令的选项和输出见 [CLI 参考](cli.md)。

1. [安装](#安装)
2. [attach 你的 Claude Code](#attach-你的-claude-code)
3. [启动 Agent](#启动-agent)
4. [发送 Message](#发送-message)
5. [运行 Job](#运行-job)
6. [Web UI](#web-ui)
7. [清理和卸载](#清理和卸载)
8. [Codex 和 herdr](#codex-和-herdr)

## 安装

需要 tmux 3.2 及以上，以及已安装并完成认证的 Claude Code 2.1.181 及以上。

使用 Homebrew（macOS，或装有 Homebrew 的 Linux）：

```sh
brew install crestai-labs/tap/feld
```

否则使用安装脚本。脚本校验适合本机的 Release archive 的 checksum 后，把 `feld` 和随附的 Plugin 放到 `~/.local/bin`（可以用 `FELD_INSTALL_DIR` 更改位置）：

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
```

然后设置 Feld。它把 PATH 上找到的 Runtime 和 Provider 的 Plugin 放到 `~/.feld/plugins/`，把 Feld 的 skill 和 status line 写入 Claude Code 的设置，把 `eval "$(feld env)"` 写入 shell 配置文件，每个值都在写入前显示，然后启动 daemon。

```sh
feld init
```

按照它打印的 `next:` 各行操作，并打开一个新的 shell。`feld daemon status` 显示 daemon 和已加载的 Plugin。

CLI 参考：[`feld init`](cli.md#feld-init)、[`feld daemon`](cli.md#daemon)

## attach 你的 Claude Code

从新的 shell 打开 tmux，并在 pane 中启动 Claude Code。

```sh
tmux new-session -s feld
claude
```

请 Claude Code：

```text
以 lead 的身份 attach 到 Feld，并带上管理其他 Actor 的 grant。
```

Feld 现在把这个 Claude Code 记录为 `lead` Actor 的 Session，你可以像以前一样继续使用它。这些 grant 让 `lead` 能在后面几步创建、启动、停止和读取其他 Actor。`feld actor get lead` 显示 Session 和 grant。

> [!NOTE]
> Claude Code 运行下面的命令。在 attach 之前，它运行的 `feld` CLI 以 Admin 的身份行事，而 Admin 是唯一能授予 grant 的 principal，所以它先带着 grant 声明 `lead`，再 attach 到这个 Actor。attach 之后，它的 `feld` 命令就以 `lead` 的身份行事。
>
> ```sh
> feld actor create lead --kind agent --runtime claude-code --no-start --grant read --grant control --grant manage-agent --grant manage-job
> feld actor attach lead --self --runtime claude-code
> ```
>
> `/feld attach lead` 会不带 grant 地 attach。要事后补上，请在终端运行 `feld actor update lead --grant read --grant control --grant manage-agent --grant manage-job`，在终端里 `feld` 又是 Admin。

CLI 参考：[`feld actor create`](cli.md#feld-actor-create)、[`feld actor attach`](cli.md#feld-actor-attach)、[`feld actor update`](cli.md#feld-actor-update)、[grant](../spec/concepts/glossary.md#grants)

## 启动 Agent

请 `lead`：

```text
在 Feld 中，在这个目录里启动一个名为 dev 的 Claude Code Agent。
```

Feld 创建 `dev` Actor，并在新的 tmux window 中为它启动 Claude Code。由 Feld 启动的 Claude Code 从一开始就带有 skill、hook 和 telemetry。接着请它：

```text
给我看看 dev 在 Feld 中做什么。
```

> [!NOTE]
> Claude Code 运行下面的命令。第一条声明 `dev` Actor，并在当前目录启动它的 Session；第二条读取该 Session 的 log。
>
> ```sh
> feld actor create dev --kind agent --runtime claude-code
> feld session log dev
> ```

要结束 Session，请 `lead` 在 Feld 中停止 `dev`；Feld 会向它的 pane 发送 `/exit`。`feld actor start dev` 用同一定义启动下一个 Session。

CLI 参考：[`feld actor create`](cli.md#feld-actor-create)、[`feld session log`](cli.md#feld-session-log)、[`feld session stop`](cli.md#feld-session-stop)

## 发送 Message

请 `lead`：

```text
在 Feld 中，给 dev 发一条 Message，请它 review 这个仓库的结构，然后把它的回复给我看。
```

Message 作为 `dev` 的下一个输入送达，`dev` 在自己的 Session 中回答。它的 `from` 是 `lead`；从终端发送的 Message，`from` 则是 `Admin`。

> [!NOTE]
> Claude Code 运行下面的命令：发送 Message，在 `dev` 的 log 中读取回复，并列出 `dev` 的 mailbox。
>
> ```sh
> feld message send dev "Review the structure of this repository"
> feld session log dev
> feld message list --to dev
> ```

CLI 参考：[`feld message`](cli.md#message)

## 运行 Job

请 `lead`：

```text
在 Feld 中，创建一个名为 hello 的 Job，打印 "Hello from Feld"，现在就运行它，并把输出给我看。
```

Job 是运行一条命令的 Actor。`--` 之后的全部内容是命令，`--start` 立即运行一个 Session，输出就是该 Session 的 log。加上 `--cron`，Job 就按 Schedule 运行。

> [!NOTE]
> Claude Code 运行下面的命令。`feld actor get hello` 列出 Session ID，该 Session 的 log 就是 Job 的输出。
>
> ```sh
> feld actor create hello --kind job --start -- sh -c 'printf "Hello from Feld\n"'
> feld actor get hello
> feld session log <session>
> ```

CLI 参考：[`feld actor create`](cli.md#feld-actor-create)、[`feld session`](cli.md#session)

## Web UI

Sign in 并在默认浏览器中打开 Web UI。URL 携带一次性的 Sign in code，5 分钟内有效。

```sh
feld ui
```

Actor、它们的 Session 和 Message、Timeline 以及 metrics 都在这里。各 view 绘制的内容见 [Web UI 视图](webui.md)。要从另一台设备打开，见 [`feld ui pair`](cli.md#feld-ui-pair)。

CLI 参考：[`feld ui`](cli.md#feld-ui)

## 清理和卸载

请 `lead`：

```text
在 Feld 中删除 hello 和 dev。
```

历史会保留，同一名字可以立即重用。

> [!NOTE]
> Claude Code 运行下面的命令。Session 仍在运行的 Actor 无法删除，所以先停止 `dev` 的 Session。
>
> ```sh
> feld session stop dev
> feld actor delete dev
> feld actor delete hello
> ```

在终端用 `feld daemon stop` 停止 daemon。

要移除 Feld，运行 `feld uninstall`。它会停止 daemon 和由 Feld 启动的 Session，还原 `feld init` 写入 Claude Code 和 Codex 设置的内容，并在询问之前列出将要移除的内容。store 和 `sessions/` 会保留给下一次 `feld init`；要把它们也移除，加上 `--purge`。最后，按照 `next:` 的各行，从 shell 配置文件中移除 `eval "$(feld env)"`，并删除 `feld` 可执行文件（通过 Homebrew 安装的话，运行 `brew uninstall feld`）。

CLI 参考：[`feld actor delete`](cli.md#feld-actor-delete)、[`feld daemon stop`](cli.md#feld-daemon-stop)、[`feld uninstall`](cli.md#feld-uninstall)

## Codex 和 herdr

**Codex**（0.156.0 及以上）

- `feld init`：在列表中勾选 `codex`。它把 skill 和 hook 写入 Codex config root（`CODEX_HOME`，未设置时为 `~/.codex`），并把 Feld 的 spool 加入 `writable_roots`。下次启动 Codex 时，它会询问是否信任 Feld 的 hook，在信任了它们的 Codex 中集成即启用。
- attach：请 Codex 运行 `feld actor attach lead --self --runtime codex`。attach 在 sandbox 之外运行，所以 Codex 询问时请批准。
- 启动：`feld actor create dev --kind agent --runtime codex --workdir "$PWD"`。model 和 approval policy 等 Codex 自身的设置，用 `--runtime-settings '{"model":"<available-model>","approval_policy":"never"}'` 传入。停止时发送 `/quit`。
- Message 成为 Codex 下一个 turn 的输入。已 attach Session 的 metrics 大多为 `unavailable`。细节见 [`spec/design/adapter/codex.md`](../spec/design/adapter/codex.md)。

**herdr**（0.9.0 及以上）

- `feld init`：在 herdr server 运行的状态下勾选列表中的 `herdr`，并在使用 Feld 期间保持 server 运行。
- Session 放在 herdr 的 workspace 中。命令不变，用 `--herdr-session` / `--herdr-unit` / `--herdr-workspace` / `--herdr-pane` / `--herdr-split` 选择放到哪里。细节见 [`spec/design/provider/herdr.md`](../spec/design/provider/herdr.md)。
