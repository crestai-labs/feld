# Quickstart

English · <a href="quickstart.zh-CN.md">简体中文</a> · <a href="quickstart.ja.md">日本語</a>

Attach the Claude Code you already use to Feld, then ask it to start another agent, send that agent a Message, and run a Job. It takes about 10 minutes.

This guide assumes Claude Code and tmux; the differences for Codex and herdr are at the [end](#codex-and-herdr). Each step says what to ask Claude Code, naming Feld so that it reaches for the `feld` CLI, and the note below it explains the commands Claude Code runs. You can run the same commands yourself from a terminal. The options and output of every command are in the [CLI reference](cli.md).

1. [Install](#install)
2. [Attach your Claude Code](#attach-your-claude-code)
3. [Start an Agent](#start-an-agent)
4. [Send a Message](#send-a-message)
5. [Run a Job](#run-a-job)
6. [Web UI](#web-ui)
7. [Clean up and uninstall](#clean-up-and-uninstall)
8. [Codex and herdr](#codex-and-herdr)

## Install

You need tmux 3.2 or later and Claude Code 2.1.181 or later, signed in.

With Homebrew (macOS, or Linux with Homebrew):

```sh
brew install crestai-labs/tap/feld
```

Otherwise, the install script places `feld` and the bundled Plugins in `~/.local/bin` (change it with `FELD_INSTALL_DIR`) after verifying the checksum of the Release archive for your machine:

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
```

Then set Feld up. It places the Plugins for the Runtimes and Providers found on PATH in `~/.feld/plugins/`, writes Feld's skill and status line into the Claude Code settings and `eval "$(feld env)"` into the shell config, showing each value before writing it, and starts the daemon.

```sh
feld init
```

Follow the `next:` lines it prints, and open a new shell. `feld daemon status` shows the daemon and the loaded Plugins.

CLI reference: [`feld init`](cli.md#feld-init), [`feld daemon`](cli.md#daemon)

## Attach your Claude Code

From the new shell, open tmux and start Claude Code in a pane.

```sh
tmux new-session -s feld
claude
```

Ask Claude Code:

```text
Attach to Feld as lead, with the grants to manage other Actors.
```

Feld now records this Claude Code as a Session of the Actor `lead`, and you keep using it as before. The grants let `lead` create, start, stop, and read other Actors in the next steps. `feld actor get lead` shows the Session and the grants.

> [!NOTE]
> Claude Code runs the following commands. Until it is attached, the `feld` CLI it runs acts as Admin, the only principal that can give grants, so it declares `lead` with the grants first and then attaches to that Actor. Once attached, its `feld` commands act as `lead`.
>
> ```sh
> feld actor create lead --kind agent --runtime claude-code --no-start --grant read --grant control --grant manage-agent --grant manage-job
> feld actor attach lead --self --runtime claude-code
> ```
>
> `/feld attach lead` attaches without grants. To add them afterwards, run `feld actor update lead --grant read --grant control --grant manage-agent --grant manage-job` from a terminal, where `feld` is Admin again.

CLI reference: [`feld actor create`](cli.md#feld-actor-create), [`feld actor attach`](cli.md#feld-actor-attach), [`feld actor update`](cli.md#feld-actor-update), [grants](../spec/concepts/glossary.md#grants)

## Start an Agent

Ask `lead`:

```text
In Feld, start a Claude Code Agent named dev in this directory.
```

Feld creates the Actor `dev` and starts Claude Code for it in a new tmux window. A Claude Code started by Feld has the skill, hooks, and telemetry from the beginning. Then ask:

```text
Show me what dev is doing in Feld.
```

> [!NOTE]
> Claude Code runs the following commands. The first declares the Actor `dev` and starts its Session in the current directory; the second reads the Session's log.
>
> ```sh
> feld actor create dev --kind agent --runtime claude-code
> feld session log dev
> ```

To end the Session, ask `lead` to stop `dev` in Feld; Feld sends `/exit` to its pane. `feld actor start dev` starts the next Session with the same definition.

CLI reference: [`feld actor create`](cli.md#feld-actor-create), [`feld session log`](cli.md#feld-session-log), [`feld session stop`](cli.md#feld-session-stop)

## Send a Message

Ask `lead`:

```text
In Feld, send dev a Message asking it to review the structure of this repository, then show me its reply.
```

The Message arrives in `dev` as its next input, and `dev` answers in its own Session. Its `from` is `lead`; a Message sent from a terminal comes from `Admin`.

> [!NOTE]
> Claude Code runs the following commands: send the Message, read the reply in the log of `dev`, and list the mailbox of `dev`.
>
> ```sh
> feld message send dev "Review the structure of this repository"
> feld session log dev
> feld message list --to dev
> ```

CLI reference: [`feld message`](cli.md#message)

## Run a Job

Ask `lead`:

```text
In Feld, create a Job named hello that prints "Hello from Feld", run it now, and show me the output.
```

A Job is an Actor that runs a command. Everything after `--` is the command, `--start` runs a Session right away, and the output is the Session's log. With `--cron`, the Job runs on a schedule.

> [!NOTE]
> Claude Code runs the following commands. `feld actor get hello` lists the Session IDs, and the log of that Session is the output of the Job.
>
> ```sh
> feld actor create hello --kind job --start -- sh -c 'printf "Hello from Feld\n"'
> feld actor get hello
> feld session log <session>
> ```

CLI reference: [`feld actor create`](cli.md#feld-actor-create), [`feld session`](cli.md#session)

## Web UI

Open the Web UI, signed in, in the default browser. The URL carries a one-time Sign in code that works for 5 minutes.

```sh
feld ui
```

The Actors, their Sessions and Messages, the Timeline, and the metrics are all there. What each view draws is in [Web UI views](webui.md). To open it from another device, see [`feld ui pair`](cli.md#feld-ui-pair).

CLI reference: [`feld ui`](cli.md#feld-ui)

## Clean up and uninstall

Ask `lead`:

```text
In Feld, delete hello and dev.
```

The history remains, and the names can be reused right away.

> [!NOTE]
> Claude Code runs the following commands, stopping the Session of `dev` first, since an Actor whose Session is running cannot be deleted.
>
> ```sh
> feld session stop dev
> feld actor delete dev
> feld actor delete hello
> ```

Stop the daemon from a terminal with `feld daemon stop`.

To remove Feld, run `feld uninstall`. It stops the daemon and the Sessions Feld started, restores what `feld init` wrote to the Claude Code and Codex settings, and lists what it removes before asking. The store and `sessions/` remain for the next `feld init`; add `--purge` to remove them too. Finally, following the `next:` lines, remove `eval "$(feld env)"` from the shell config and the `feld` executable (`brew uninstall feld` for a Homebrew install).

CLI reference: [`feld actor delete`](cli.md#feld-actor-delete), [`feld daemon stop`](cli.md#feld-daemon-stop), [`feld uninstall`](cli.md#feld-uninstall)

## Codex and herdr

**Codex** (0.156.0 or later)

- `feld init`: check `codex` in the list. It writes the skill and hooks to the Codex config root (`CODEX_HOME`, or `~/.codex`) and adds the Feld spool to `writable_roots`. The next Codex asks whether to trust Feld's hooks; the integration works from a Codex that trusted them.
- Attach: ask Codex to run `feld actor attach lead --self --runtime codex`. attach runs outside the sandbox, so approve it when Codex asks.
- Start: `feld actor create dev --kind agent --runtime codex --workdir "$PWD"`. Pass Codex's own settings, such as the model and the approval policy, with `--runtime-settings '{"model":"<available-model>","approval_policy":"never"}'`. Stopping sends `/quit`.
- Messages become the input of Codex's next turn. Metrics of an attached Session are mostly `unavailable`. Details are in [`spec/design/adapter/codex.md`](../spec/design/adapter/codex.md).

**herdr** (0.9.0 or later)

- `feld init`: with the herdr server running, check `herdr` in the list, and keep the server running while using Feld.
- Sessions are placed in herdr workspaces. The commands stay the same; `--herdr-session` / `--herdr-unit` / `--herdr-workspace` / `--herdr-pane` / `--herdr-split` choose where. Details are in [`spec/design/provider/herdr.md`](../spec/design/provider/herdr.md).
