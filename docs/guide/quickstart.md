# Quickstart

Install Feld and handle Claude Code on tmux from Feld. It takes about 10 minutes.

This guide assumes Claude Code and tmux. Differences for Codex and herdr are noted where they apply. The options and output of each section's commands are in the [CLI reference](cli.md). Other user documents are in the [Guide](README.md), and the source of truth for contracts is in [`docs/spec/`](../spec/).

1. [Prerequisites](#prerequisites)
2. [Install](#install)
3. [Initial setup](#initial-setup)
4. [Attach a manually started Claude Code](#attach-a-manually-started-claude-code)
5. [Start Claude Code from Feld](#start-claude-code-from-feld)
6. [Send a Message](#send-a-message)
7. [Create and run a Job](#create-and-run-a-job)
8. [Web UI](#web-ui)
9. [Clean up](#clean-up)
10. [Uninstall](#uninstall)

## Prerequisites

- tmux 3.2 or later (herdr 0.9.0 or later)
- Claude Code 2.1.181 or later (Codex 0.156.0 or later), installed and authenticated

## Install

With Homebrew (macOS, or Linux with Homebrew):

```sh
brew install crestai-labs/tap/feld
```

Otherwise, the install script fetches the archive for this machine from GitHub Releases, verifies the checksum, and places `feld` and the bundled Plugins in `~/.local/bin`. Choose the location with `FELD_INSTALL_DIR` and the version with `FELD_VERSION`.

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
```

Add `~/.local/bin` to PATH if it is missing. To build from source, see [CONTRIBUTING.md](../../CONTRIBUTING.md#development-setup).

## Initial setup

Run `feld init` in a terminal and answer the questions. It creates `FELD_HOME` (default `~/.feld`), places the Plugins chosen from the bundled ones in `~/.feld/plugins/`, writes the Claude Code and shell settings, and starts the daemon. `--yes` is for scripts and agrees to everything without asking. The first time, answer in the terminal and check what it rewrites.

```sh
feld init
```

1. **Choose Plugins**: move between items with `↑` `↓`, toggle a check with `Space`, and confirm with `Enter`. Plugins for the Runtimes (`claude`) and Providers (`tmux`) found on PATH are checked from the start
2. **Claude Code settings**: the `claude-code` Runtime Adapter writes Feld's skill and Claude Code plugin to `skills/feld/` in the Claude Code config root (`CLAUDE_CONFIG_DIR`, or `~/.claude` if unset), and points `otelHeadersHelper` and `statusLine` in `settings.json` in the same root to it. For these two, it shows the current values and the values to write and asks for consent. If you have your own `statusLine`, its command stays in `--chain` of Feld's status line helper, and the status line shows your output alongside Feld's section. Your own `otelHeadersHelper` is replaced, and the original value is kept in `~/.feld/prepared/claude-code.json`
3. **Shell settings**: appends `eval "$(feld env)"` to the shell config file (`~/.zshrc` for zsh, `~/.bashrc` for bash). Claude Code started from a shell that loaded this line sends telemetry to the daemon's Collector
4. **Start the daemon**: the daemon starts the Control API and Runtime API, the Web UI, and the OTel Collector, and loads the Plugins in `~/.feld/plugins/`

On completion, `next:` shows the commands to run in order, `ui:` the Web UI URL, and `note:` notes per Runtime. Following `next:`, `source` the config file or open a new shell.

Check the daemon status with the following command. `runtimes:` and `providers:` show the loaded Plugins. Start a stopped daemon with `feld daemon start`. To review the environment, `feld doctor` shows the result and fix for each check in one line.

```sh
feld daemon status
```

> [!NOTE]
> **Codex**: also check `codex` in the list. The `codex` Runtime Adapter writes the skill and hook declarations to the config root (`CODEX_HOME`, or `~/.codex` if unset), and adds the Feld home `spool/shared` to `writable_roots` in `config.toml`. The next time Codex starts, it asks whether to trust Feld's hooks, and integration is enabled from a Codex that trusted them. Authenticate with Codex's own login. Details on the sandbox and spool are in [`spec/design/adapter/codex.md`](../spec/design/adapter/codex.md)
>
> **herdr**: with the herdr server running, check `herdr` in the list. The herdr API reaches only a running server, so keep the server running while using Feld. The following commands stay the same, and Sessions are placed in herdr workspaces. Use `--herdr-session` / `--herdr-unit` / `--herdr-workspace` / `--herdr-pane` / `--herdr-split` to specify which unit of which server to place them in
>
> **Placing from a script**: pass the answers with `--runtime` / `--provider`. To place two of the same kind, pass the flag twice. A `feld init` that changed Plugins asks whether to restart the running daemon

CLI reference: [`feld init`](cli.md#feld-init), [`feld daemon`](cli.md#daemon), [`feld doctor`](cli.md#feld-doctor), [launchSpec options](cli.md#launchspec-options)

## Attach a manually started Claude Code

Open tmux from a shell that loaded `eval "$(feld env)"`, and start Claude Code in a pane.

```sh
tmux new-session -s feld-claude
claude
```

Ask Claude Code to run the following command. Running `/feld attach` inside Claude Code attaches with an automatically generated name, and `/feld attach <ACTOR>` attaches as `<ACTOR>`.

```sh
feld actor attach manual-claude --self --runtime claude-code
```

Feld records the running Claude Code as an attached Session of the `manual-claude` Actor. The user continues to start and exit Claude Code. The skill, hooks, and Plugin Monitor take effect for Claude Code started after `feld init` (for Claude Code already open, after `/reload-plugins`), and telemetry takes effect for Claude Code started after `feld init`. Claude Code without this preparation can also be attached, and the `note:` line in the output points to `feld init`. For Claude Code started from a shell without `feld env`, the `telemetry` line in the attach output shows metrics as `unavailable` with the reason.

Check the attach result from another terminal.

```sh
feld actor get manual-claude
```

> [!NOTE]
> **Codex**: ask Codex to run `feld actor attach manual-codex --self --runtime codex`. attach is an Admin operation, so it runs outside the sandbox. `feld` inside the sandbox is rejected, and Codex asks for approval to run outside the sandbox; approving it lets the attach through. For a Codex that does not ask for approval (`approval_policy` is `never`), run the command attached to the rejection, which points to the pane showing Codex, in a shell outside the sandbox. Metrics for an attached Session are `unavailable`, except for the context window utilization the hook sends

CLI reference: [`feld actor attach`](cli.md#feld-actor-attach)

## Start Claude Code from Feld

Create the `claude-dev` Agent. On creation, the Agent starts Claude Code on tmux.

```sh
feld actor create claude-dev --kind agent --runtime claude-code --workdir "$PWD"
```

For the started Claude Code, the `claude-code` Runtime Adapter prepares per-Session settings and a Claude Code plugin, and passes the skill, hooks, Plugin Monitor, and telemetry settings from startup. Read the Session's conversation with the following command.

```sh
feld session log claude-dev
```

Request a Session that Feld started to end with the following command.

```sh
feld session stop claude-dev
```

Feld sends `/exit` to the Claude Code pane, plus the answer to the exit confirmation when tasks are running, and the Session ends. `feld actor start claude-dev` starts the next Session, and `feld actor restart claude-dev` stops the current Session and replaces it with the next one. To end an attached Session, exit the attached Claude Code itself.

> [!NOTE]
> **Codex**: create it with `--runtime codex`. The model is Codex's default; pass Codex's own settings such as the model and approval policy to `--runtime-settings` as `config.toml` items. For `<available-model>`, use a name listed by `/model` after starting Codex. On an end request, Feld sends `/quit` to the Codex pane
>
> ```sh
> feld actor create codex-dev --kind agent --runtime codex --workdir "$PWD"
> feld actor update codex-dev --runtime-settings '{"model":"<available-model>","approval_policy":"never"}'
> feld actor restart codex-dev
> ```
>
> Codex waits for input immediately in a workdir marked trusted in `config.toml`, and in other workdirs it shows a trust confirmation and waits. For a repo where the workdir's `.codex/` settings and hooks may take effect without confirmation, declare it with `--trust-workdir`
>
> ```sh
> feld actor create codex-dev --kind agent --runtime codex --workdir "$PWD" --trust-workdir
> ```

CLI reference: [`feld actor create`](cli.md#feld-actor-create), [`feld session log`](cli.md#session), [`feld session stop`](cli.md#feld-session-stop), [launchSpec options](cli.md#launchspec-options)

## Send a Message

From another terminal, send a Message to Claude Code. The recipient can be the attached `manual-claude` or the `claude-dev` that Feld started.

```sh
feld message send claude-dev "Review the structure of this repository"
```

In Claude Code with the Plugin Monitor enabled, the Message is delivered to the Session automatically. In a Session where the Plugin Monitor is not yet enabled, such as Claude Code open since before `feld init`, ask Claude Code to run the following command to receive the Message.

```sh
feld self message recv
```

Check the Actor's mailbox from the outside terminal. The `from` of a Message sent from the outside terminal is `Admin`.

```sh
feld message list --to claude-dev
```

> [!NOTE]
> **Codex**: the queue helper puts the Message into Codex's queue, and it becomes the input of the next turn

CLI reference: [`feld message`](cli.md#message), [`feld self`](cli.md#self)

## Create and run a Job

Create the `hello` Job and start it right away.

```sh
feld actor create hello --kind job --start -- sh -c 'printf "Hello from Feld\n"'
```

Everything after `--` becomes the Job's command. A Job starts a Session on creation only with `--start`. The Job definition and Session are recorded in the `default` Workspace.

Check the Actor list, the Job details, and the records Feld left.

```sh
feld actor list -a
feld actor get hello
feld record list --actor hello
```

Session IDs are listed under `Sessions` in `feld actor get`. Read the Job output by passing the Session ID.

```sh
feld session log <session>
```

To start a new Session with the same definition, run the following.

```sh
feld actor start hello
```

`spawned_by Admin` in `feld session get` and `by Admin` in `feld record list` refer to operations from this terminal. CLI operations using the daemon's local token are recorded as [Admin](../spec/concepts/glossary.md#admin). Admin is a principal outside the Actors, and only Actors with a mailbox can be Message recipients.

With `--cron`, it becomes a Job with a Schedule; `feld actor pause` / `feld actor resume` stop and resume the Schedule.

```sh
feld actor create nightly --kind job --cron '0 9 * * *' --timezone Asia/Tokyo -- ./nightly.sh
```

CLI reference: [`feld actor`](cli.md#actor), [`feld session`](cli.md#session), [`feld record`](cli.md#record)

## Web UI

Sign in and open the Web UI in the default browser.

```sh
feld ui
```

To print only the URL, run the following.

```sh
feld ui --print-url
```

The URL fragment contains a one-time Sign in code. Open it in that browser within 5 minutes.

The daemon responds only to requests sent to loopback and to names allowed with `--allow-host`. To open it from a browser on another device, relay the daemon's Port through a reverse proxy, and start the daemon with that host name allowed (`feld daemon restart --allow-host <host>`). On the Sign in screen of the Web UI opened in that browser, enter the code that `feld ui pair` prints on the daemon's machine.

```sh
feld ui pair --name laptop
```

> [!NOTE]
> Tailscale is the assumed reverse proxy. Relay the daemon's Port with `tailscale serve --bg <PORT>`, and allow the name shown by `tailscale serve status` (`<host>.<tailnet>.ts.net`) with `--allow-host`.

List signed-in browsers with `feld browser list`, and sign one out with `feld browser revoke laptop`. Sign out in Settings signs out that browser itself.

What each view draws is in [Web UI views](webui.md).

CLI reference: [`feld ui`](cli.md#feld-ui)

## Clean up

Delete Actors you are done with. The history remains, and the same name can be reused right away. Stop an Actor whose current Session is running with `feld session stop` first.

```sh
feld actor delete hello
```

Stop the daemon.

```sh
feld daemon stop
```

CLI reference: [`feld actor delete`](cli.md#feld-actor-delete), [`feld daemon stop`](cli.md#feld-daemon-stop)

## Uninstall

To remove Feld, run `feld uninstall`. It stops the daemon and the Sessions Feld started, restores what `feld init` wrote to the Claude Code and Codex config roots, and removes the settings from Feld home. It lists what it removes before asking.

```sh
feld uninstall
```

`sessions/` and the store remain, so running `feld init` again lets you read the same history. To remove them too, add `--purge`. Finally, following the `next:` lines, remove `eval "$(feld env)"` from the shell config file and the `feld` executable (`brew uninstall feld` for a Homebrew install).

CLI reference: [`feld uninstall`](cli.md#feld-uninstall)
