# CLI reference

This page shows the arguments, options, behavior, and output of each `feld` command. The arguments and options match each command's `--help`, and the clap definitions live in `crates/feld-cli/src/args.rs`.

The CLI is a thin client that calls the daemon API ([daemon-api.md](daemon-api.md)). People, scripts, and Agents running inside a Session use the same entry point and call the same API as the Web UI. Commands print the facts Feld committed as records, and the CLI adds no inference of its own. Concepts and terms follow the [glossary](../spec/concepts/glossary.md).

Output examples show the human-readable output in its non-TTY form. `--json` outputs the same content as JSON.

## Contents

- [feld](#feld)
- [Common conventions](#common-conventions)
- [feld init](#feld-init)
- [feld uninstall](#feld-uninstall)
- [feld doctor](#feld-doctor)
- [daemon](#daemon): [start](#feld-daemon-start) / [status](#feld-daemon-status) / [stop](#feld-daemon-stop) / [restart](#feld-daemon-restart)
- [workspace](#workspace): [list](#feld-workspace-list) / [create](#feld-workspace-create) / [delete](#feld-workspace-delete) / [get](#feld-workspace-get)
- [actor](#actor): [create](#feld-actor-create) / [update](#feld-actor-update) / [delete](#feld-actor-delete) / [get](#feld-actor-get) / [list](#feld-actor-list) / [start](#feld-actor-start) / [attach](#feld-actor-attach) / [restart](#feld-actor-restart) / [pause](#feld-actor-pause) / [resume](#feld-actor-resume)
- [session](#session): [get](#feld-session-get) / [list](#feld-session-list) / [stop](#feld-session-stop) / [kill](#feld-session-kill) / [observe](#feld-session-observe) / [log](#feld-session-log) / [metrics](#feld-session-metrics)
- [record](#record): [list](#feld-record-list)
- [message](#message): [send](#feld-message-send) / [delete](#feld-message-delete) / [get](#feld-message-get) / [list](#feld-message-list)
- [runtime](#runtime): [list](#feld-runtime-list) / [metrics](#feld-runtime-metrics)
- [provider](#provider): [list](#feld-provider-list)
- [self](#self): [get](#feld-self-get) / [token](#feld-self-token) / [message recv](#feld-self-message-recv)
- [ui](#ui): [feld ui](#feld-ui) / [pair](#feld-ui-pair)
- [browser](#browser): [list](#feld-browser-list) / [revoke](#feld-browser-revoke)
- [feld env](#feld-env)
- [launchSpec options](#launchspec-options)
- [Internal commands](#internal-commands)

## feld

Agent Loop Control Plane. Commands take the form `feld <noun> <verb> [target] [options]`, and most connect to the daemon of the Feld home. The nouns and verbs come from the glossary's [core objects](../spec/concepts/glossary.md#core-objects) and the verbs of the [feature list](../spec/concepts/daemon-features.md).

```text
feld [OPTIONS] <COMMAND>
```

**Commands**

- `init`: Create the Feld home, install the bundled Plugins, and start the daemon
- `uninstall`: Remove what `feld init` and the daemon installed
- `doctor`: Check the environment Feld runs in, one line per check
- `daemon`: Run and inspect the daemon that holds the store and serves the API
- `workspace`: Group Actors into Workspaces. Actor names are unique per Workspace
- `actor`: Create, change, and read the Actors of a Workspace
- `session`: Read and operate Sessions, the runs of an Actor
- `record`: Read the facts Feld itself recorded
- `message`: Send, delete, read, and list Messages between Actors. Alias `msg`
- `runtime`: Read the installed Runtime Adapters and their Runtime metrics
- `provider`: Read the installed Execution Providers
- `self`: Use the Runtime API as the Session this command runs in
- `ui`: Sign in and open the Workspace's Feld Web UI in a browser, and issue a code that lets a browser on another device Sign in
- `browser`: List the browser tokens of signed-in browsers, and revoke them to Sign them out
- `env`: Print the environment that ties a shell to the daemon of the Feld home

Commands that have `list` also accept `ls`.

**Global Options** (available on every subcommand)

- `--home <PATH>`: Feld's state directory. Holds the store, the daemon, Plugins, and the local API token. Environment variable `FELD_HOME`. Default `~/.feld`
- `--workspace <KEY>`: The Workspace to operate on. Accepts values matching `^[a-z0-9][a-z0-9-]*$`. Environment variable `FELD_WORKSPACE` (empty is treated as unset). Default `default`
- `--json`: Write results as JSON to standard output, and errors as `{"error":{"kind","message","next"}}` to standard error
- `-h, --help`: Help. `--help` shows the description, arguments, options, examples, and exit codes; `-h` shows a summary
- `-V, --version`: Version, build time (UTC), and the Git branch at build time (when built on a branch). Only `feld` accepts it

## Common conventions

**Arguments**

- `<ACTOR>` is the name of an Actor in the `--workspace` Workspace. Actor names consist of lowercase letters, digits, and `-`, and start with a letter or digit. Names that do not match are rejected
- `<TARGET>` is a Session ID (`session_...`) or an Actor name. An Actor name refers to its current Session. For read commands (`session get` / `log` / `metrics`), it refers to the latest Session when there is no current Session
- `<MESSAGE>` is a Message ID (`message_...`)
- `<TIME>` is an RFC 3339 timestamp. `--start <TIME> --end <TIME>` gives the range, and `--bucket <SECONDS>` is the width of a time-series bin
- Sessions Feld starts receive their own Workspace as `FELD_WORKSPACE`

**Output**

- `--json` combined with `--follow` writes one JSON object per line (JSON Lines), one at a time
- Time: human-readable output shows record times in the daemon's local timezone to the second (`2026-09-13T10:42:09+09:00`). `--json` returns the committed values as they are
- Color and symbols: human-readable output maps the terminal's 16 colors to roles. accent (bold bright blue, Feld's color) marks the wordmark and the step being answered, ok (green) marks success and values being written, warn (yellow) marks caution, error (red) marks rejections, failures, and values being replaced, and dim (gray 242 on 256-color terminals, bright black otherwise) marks rules, headings, keys, and hints; values keep the plain color. There are 10 symbols, `∿ ◇ ◆ │ └ ❯ ◉ ○ ✓ ✗`, stacked from the left edge without aligning line ends, so the layout reads the same on terminals where a symbol takes two columns. When the locale is not UTF-8, ASCII (`~ - * | - > [x] [ ] + x`) is used instead. When stdout is not a TTY, when `NO_COLOR` is set, when `TERM` is empty or `dumb`, and with `--json`, neither color nor symbols are used, and output is plain `key: value`. `CLICOLOR_FORCE` makes color and symbols apply even to a non-TTY stdout
- Version mismatch: when the running daemon's version differs from `feld` (for example after reinstalling Feld), commands that talk to the daemon print the single line `warning: daemon 0.1.0 differs from this CLI 0.1.1 · next: feld init` to standard error and continue. Even with `--json`, standard output and the exit code stay the same. The version is read from `version` in the daemon's status. `init`, `daemon stop`, and `daemon restart`, which align the versions, and `doctor`, which reports it as a check, do not print it

**Writes to the Feld home**

The commands that start the daemon (`init`, `daemon start`, `daemon restart`, `daemon run`) create the Feld home and set the directory to `700` and `daemon.lock` to `600`. The other commands that write to the Feld home are `uninstall`, `daemon stop` (the lifecycle lock), `actor attach` (self index, Session token, Runtime config preparation), `ui` (the `admin/ui-pair/` page it has the browser open), and the internal command `program-capture` (`sessions/<id>/console/`), which the daemon starts to keep a Session's output. The remaining commands only read the Feld home, so they work with read permission on it alone. When going through the [spool](../spec/design/spool.md) from inside a sandbox, they write the request file to their own Session's directory in `spool/`. When the Feld home or `daemon.lock` is missing, they report that the daemon is not running.

**Caller identification and token resolution**

Commands that use the daemon resolve a token in this order: `FELD_SESSION_TOKEN`, the self index of the caller's Execution Provider, the self index of the caller's Runtime Adapter, and `FELD_HOME/admin/api-token` (the local token) ([resolution order in auth.md](../spec/design/auth.md#runtime-adapter-resolution-order)). With the first three, the caller is treated as the Session's Actor; with `api-token`, as [Admin](../spec/concepts/glossary.md#admin). `actor attach`, `ui`, `browser`, and `daemon` always use `api-token`, and `self` uses only the first three. A Message's `from` and the `spawner` of a [spawned](../spec/concepts/glossary.md#spawned) Session are determined from this, and the CLI cannot forge them.

When the Runtime's sandbox rejects TCP, commands go through the [spool](../spec/design/spool.md#choosing-the-path). A call without a Session token (an Admin operation) tells the user to run it outside the sandbox and ends with exit code 1.

**Exit codes**

The exit code reports whether the CLI request succeeded. Scripts use the exit code to decide whether to proceed, and read a Session's outcome (`failed`, `unknown`, and so on) from `result` in the output (the value with `--json`).

- `0`: Feld accepted the request, or answered a read. Also 0 when an already requested call returns the existing record, and for `--help` and `--version`. When the reader of stdout closes first (`| head` and the like), output stops there with 0. [`feld self message recv`](#feld-self-message-recv), which records the read, is the exception: it returns 2 when writing the output fails
- `1`: `rejected`. The daemon or the CLI rejected the request (admission, kind mismatch, stop / kill / restart of an `attached` Session, delete of an Actor with a running current Session, missing target, name grammar, and so on). Also 1 when `actor create` commits only the declaration and the start is rejected, and when a Session never started (`lifecycle.startFailed`); the output says so
- `2`: `operational`. Invalid arguments, the daemon is unreachable, or a local step failed. Also 2 when the process holding the daemon lock does not answer a status request within 5 seconds; instead of waiting, the command shows the pid and API from `daemon.json` and the remedy of stopping it with `kill <pid>` and starting again with `feld daemon start`. Long-waiting requests such as `self message recv --wait` have no such limit

**Errors and `next:`**

Errors print `Error: <cause>` to standard error, and when a next step can be given, add `next: <next step>` on the following line. In `--json`, `kind` is `rejected` for exit code 1 or `operational` for 2, and `next` is the next step (null if none). When the daemon's rejection has a `next`, that is shown ([daemon-api.md](daemon-api.md#common-conventions)). When [`feld doctor`](#feld-doctor) shows the same remedy, the same wording is used. `next:` is added in these situations.

| Situation | `next:` |
|---|---|
| The daemon is not running (including `feld ui`) | `feld daemon start` (`feld init` if there is no Feld home) |
| In `feld init`, a bundled Plugin's executable is not next to `feld` | Reinstall Feld, putting `feld` and `feld-plugin-<name>` in the same directory |
| In `feld init`, neither tmux nor herdr is on PATH | Install tmux or herdr |
| When starting a Session, the Provider (`tmux`, `herdr`) or Runtime (`claude`, `codex`) executable is not on the daemon's PATH (the daemon rejects with 409) | Install it (and log in, for a Runtime), or use another Provider |
| The Workspace does not exist | `feld workspace create <key>` (list with `feld workspace list`) |
| `feld actor attach --self` cannot find the pane running the Runtime (outside a pane, the foreground is not the Runtime, the Provider cannot be narrowed to one) | Run it inside the pane running the Runtime, and `feld doctor` |
| The Runtime of `feld actor attach --self` runs the conversation in a process outside the pane (Codex's app-server), so the pane cannot be determined | Ask the person for the pane and redo the attach with it |
| `--provider` is omitted and the Provider cannot be narrowed to one | `--provider <name>` listing the installed Providers |

## feld init

Make Feld ready to use. It has five steps: create `FELD_HOME`, choose per kind which of the [Plugins](../spec/design/plugin.md) bundled with Feld to use and place them in `$FELD_HOME/plugins/<kind>/<name>/`, prepare the Runtime people start themselves for each chosen Runtime Adapter, append a line loading [`feld env`](#feld-env) to the shell config file, and start the daemon.

```text
feld init [OPTIONS]
```

**Options**

- `--runtime <NAME>`: Runtime Adapter to install. `claude-code` or `codex`. Can be given multiple times
- `--provider <NAME>`: Execution Provider to install. `tmux` or `herdr`. Can be given multiple times
- `-y, --yes`: Without asking, install the Plugins found on PATH, consent to changing the Runtime config and appending to the shell config file, and start the daemon

On a TTY, it prints the `∿ Feld v0.2.0` wordmark first and stacks the steps along a `│` rule. Only the step being answered is open with `◆`; answered steps collapse to a single `◇ heading  answer` line. Steps that need no question appear collapsed from the start. The final `└` line shows whether Feld is ready (`✓` / `✗`) and the next step.

```console
$ feld init
∿ Feld v0.2.0

◇ Feld home  /Users/alice/.feld
│
◆ Choose the Plugins to install  ↑↓ move · space check · enter confirm
│
│  Runtime Adapter
│  ❯ ◉ claude-code   Claude Code
│    ○ codex         Codex
│
│  Execution Provider
│    ◉ tmux          tmux
│    ○ herdr         herdr
└
```

When not on a TTY, it prints the collapsed form as plain `key: value`.

```console
$ feld init --yes | cat
Feld v0.2.0
Feld home:   /Users/alice/.feld
Plugins:     runtimes: claude-code  providers: tmux
Claude Code: prepared · otelHeadersHelper replaced · statusLine chained
Shell:       added eval "$(feld env)" to /Users/alice/.zshrc
Daemon:      started · pid 48213 · http://127.0.0.1:7381
Feld is ready.
  next:  source /Users/alice/.zshrc
  next:  feld ui
  ui:    http://127.0.0.1:7381/ui/  ·  first time in this browser: run `feld ui` to sign in
  note:  Claude Code: run /reload-plugins in a Claude Code that is already open
  note:  Claude Code: restart a Claude Code started before this init to send its metrics
  note:  run `feld doctor` if something does not work
```

**Choosing Plugins**

The **detected set** is the bundled Plugins whose corresponding executable is found on the machine's PATH. The executables are `claude` for `claude-code`, `codex` for `codex`, `tmux` for `tmux`, and `herdr` for `herdr`.

- When no Runtime is found, the detected Runtime is `claude-code`, and a `note:` line under `Feld is ready.` says that `claude` was not found on PATH
- When no Provider is found and there is neither `--provider` nor an installed Provider, init installs nothing and ends with exit code 1 and an error guiding the user to install tmux or herdr. This is decided before asking, so it is the same on a TTY and with `--yes`

On a TTY, it asks with a list headed per kind. `↑` `↓` move between items, `Space` toggles a check, and `Enter` confirms. The initial checks are the installed Plugins, or the detected set if none are installed yet. `Enter` confirms when each kind being asked has at least one check; when a kind has none, a line under its heading asks to choose at least one and the list stays open. `Ctrl-C`, `Ctrl-D`, `Esc`, and `q` close the list and end with exit code 1 without installing anything.

`--runtime` / `--provider` give that answer as flags: kinds with flags are used as given, and the remaining kinds are asked. `--yes` uses the detected set. When not on a TTY and with `--json`, init does not ask: kinds with flags use the flags, when only one kind has flags the remaining kind is empty, and when no kind has flags the detected set is the answer.

The collapsed `Plugins` line shows the installed Plugins per kind after installation. Rerunning reinstalls the chosen Plugins and keeps unchosen installed Plugins as they are, marked `(kept)`. To remove a Plugin, delete its directory from `$FELD_HOME/plugins/`. Non-bundled Plugins go in a directory of the same shape under `$FELD_HOME/plugins/`. A kind with no installed Plugin shows `(none)`, the next step after `Feld is not ready.` becomes the `feld init` that installs that kind's default, and init ends there.

When the product of an installed bundled Plugin is on PATH and reports a version older than the lower bound of the [supported versions](../spec/design/plugin.md#supported-versions) (tmux 3.2, herdr 0.9.0, Claude Code 2.1.181, Codex 0.156.0), the `Plugins` step adds one line per product in the form `warning: tmux 3.1c is not supported; the tmux Provider needs tmux 3.2 or later`, and init continues. `--json` puts this text in the `warnings` array.

**Preparing Runtimes**

After installing the Plugins, the Adapter's `prepare` ([plugin.md](../spec/design/plugin.md#operation)) writes Feld's skill and settings to the config root of the Runtime people start themselves (`CLAUDE_CONFIG_DIR` or `~/.claude` for Claude Code, `CODEX_HOME` or `~/.codex` for Codex) ([Claude Code](../spec/design/adapter/claude-code.md#preparing-for-attach), [Codex](../spec/design/adapter/codex.md#preparing-for-attach)). Each Runtime has its own step.

1. Call `prepare` with `apply: false`
2. If there are `chain`, `replace`, or `add` entries, ask for consent. The consent step shows the config path to the right of the heading, and for each entry that is not `keep`, lists the key followed by a `-` line (the current value, red) and a `+` line (the value to write, green). A dim sentence to the right of the key says what happens to the current value. An entry with no current value has a single `+` line. Long lines are not truncated and wrap at the terminal. On a TTY, it asks `◉ Yes  ○ No`; `y` / `n` answer immediately, `←` `→` select, and `Enter` confirms. No, `Ctrl-C`, `Esc`, and `q` keep that Runtime's config as it is and show `not prepared (declined)`. `--yes` counts as consent. When not on a TTY and without `--yes`, that Runtime's config stays as it is, the step shows the same `-` / `+` lines and `` run `feld init --yes` to apply them ``, and init ends with `Feld is not ready.` and exit code 1
3. With consent, or when no entry needs consent, call `prepare` with `apply: true`. The collapsed line shows `prepared`, followed by the `chain`ed and `replace`d keys joined with `·`. The Adapter's `notes` appear in the final `note:` lines, prefixed with the Runtime name

For Claude Code, the person's `statusLine` stays in the `--chain` of Feld's status line helper, and the person's replaced `otelHeadersHelper` stays in the prepare record (`$FELD_HOME/prepared/claude-code.json`). Codex keeps the prepared config root in `$FELD_HOME/prepared/codex.json`. When a prepare record cannot be read, init fails showing its path, without changing the Runtime config or the Feld home records.

A Runtime with no entry needing consent completes with a single `prepared` line without asking. Rerunning follows the same steps, and when every entry is `keep`, nothing is asked.

```console
◆ Claude Code  /Users/alice/.claude/settings.json
│
│  skills/feld/
│  + /Users/alice/.claude/skills/feld
│
│  otelHeadersHelper  the current value is kept in Feld's prepare record
│  - /Users/alice/.claude/otel-headers.sh
│  + /Users/alice/.claude/skills/feld/bin/otel-headers
│
│  statusLine  the current command keeps running, chained behind Feld's
│  - /Users/alice/.claude/statusline.sh
│  + /Users/alice/.claude/skills/feld/bin/status-line --chain '/Users/alice/.claude/statusline.sh'
│
│  Apply these changes to your Claude Code settings?  ◉ Yes  ○ No
└
```

**Shell config file**

Metrics from an attached Claude Code arrive only from a Claude Code started in a shell that loaded `eval "$(feld env)"` ([Claude Code](../spec/design/adapter/claude-code.md#preparing-for-attach)). init appends this line, together with a comment line saying Feld added it, to the end of the file an interactive shell reads every time. The target is `~/.zshrc` when `$SHELL` is zsh (`$ZDOTDIR/.zshrc` when `ZDOTDIR` is set) and `~/.bashrc` for bash, created if missing.

```sh
# Added by `feld init`: connects this shell to the Feld daemon
eval "$(feld env)"
```

- When the target, or a config file that shell reads (`.zprofile` in the same directory as the target for zsh, `~/.bash_profile` and `~/.profile` for bash), has a line running `feld env` with `eval` (excluding `eval` inside comments or aliases), init does not append and shows `already in <path>`
- On a TTY, the target file appears to the right of the heading, the two lines to append are listed as `+` lines, and init asks `Add this line to your shell settings?` with `◉ Yes  ○ No`. `--yes` counts as Yes. On No, and when not on a TTY without `--yes`, init does not append
- The collapsed line is one of `added eval "$(feld env)" to <path>`, `eval "$(feld env)" already in <path>`, `not added (declined)`, `not added (needs your consent)`, and `not added (<shell> is not zsh or bash)`
- A run that ends with `Feld is not ready.` skips this step
- When init does not append, `Feld is ready.` is unchanged, and a `note:` line guides the user to add `eval "$(feld env)"` to the shell config file
- `--json` returns in `shell` the target `path` (null for shells other than zsh and bash) and the result `status` (`added`, `present`, `declined`, `needs-consent`, `unsupported`)

[`feld uninstall`](#feld-uninstall) does not remove this line; it shows it in a `next:` line.

**Starting the daemon**

On startup, the daemon aligns the installed bundled Plugins with its own version before loading Plugins, so init asks a single final question about running the daemon.

- When the daemon is not running, it asks `Start the daemon now?`
- When the daemon is running, it asks `Restart the daemon to load the Plugins?` so the daemon loads the Plugin changes, noting that Sessions keep running on their Providers

The default is Yes, and `--yes` counts as Yes. The result collapses to one line, `started · pid · api` (`restarted` on restart), and the next step becomes `feld ui`. On No, and when not on a TTY without `--yes`, the daemon is left as it is, and the next step after `Feld is ready.` shows `feld daemon start` (`feld daemon restart` if running). `Feld is ready.` means the Plugins and Runtimes are prepared. When startup fails, the step becomes `✗`, shows the reason and guidance to run `feld daemon start`, and init ends with exit code 1. `--json` leaves the daemon as it is and returns the next step in `next` and the Web UI URL of the running daemon in `ui` (null if not running).

**Output on completion**

Under `Feld is ready.`, init lists these lines.

- `next:` shows only the commands the person types, one per line in the order to type them, in a form that runs as-is in a shell (paths quoted as needed). When the shell config file has a line loading `feld env` and the environment init ran in lacks the values of `feld env`, `source <that file>` comes first. Then comes the daemon step (`feld ui`, `feld daemon start`, or `feld daemon restart`)
- `ui:` appears when init started or restarted the daemon, showing the Web UI URL (`web_ui` of `feld daemon status`) and that the first time in a browser, the person signs in with `feld ui`. It does not show a pairing code
- `note:` shows one supplementary note per line. Runtime Adapter `notes` are prefixed with the Runtime name. The last line suggests running [`feld doctor`](#feld-doctor) if something goes wrong

**Checks on failure**

When init ends in failure (a Provider missing from PATH, a missing Plugin executable, a failed Runtime preparation, a failed daemon start, and so on), it shows the failure, then runs the same checks as [`feld doctor`](#feld-doctor) and shows the results in the same form under a `feld doctor` heading. The exit code is that of the failure. The checks do not run when the user cancels the selection, gives the name of a Plugin that is not bundled, or init ends waiting for consent. `--json` returns the failure on stderr and the check results on stdout in the same form as `feld doctor --json`.

## feld uninstall

Remove what [`feld init`](#feld-init) and the daemon placed in the person's environment. `FELD_HOME` is determined the same way as for `feld init`.

```text
feld uninstall [OPTIONS]
```

**Options**

- `--purge`: Also remove `sessions/` and the store, and remove the emptied Feld home
- `-y, --yes`: Consent to everything in the list without asking

The target is a directory that bears a Feld home marker (one of `admin/api-token`, a regular file Feld writes that only the owner can read; an empty `daemon.lock`; or the SQLite `admin/feld.db`, including the top-level `api-token` and `feld.db` from before the daemon moved them to `admin/`); without a marker, nothing is removed and the command ends with exit code 1. The steps are as follows.

1. If the daemon is running, collect the `running` Sessions per Workspace. `owned` Sessions are killed, and `attached` Sessions remain because they are processes the person started. If the daemon is stopped, open the store read-only as [`feld doctor`](#feld-doctor) does. If there are open Sessions, show their list and ``run `feld daemon start`, then `feld uninstall` again``, and fail without changing anything
2. For each installed Runtime Adapter, call `restore` ([plugin.md](../spec/design/plugin.md#operation)) with `apply: false` and get, from the prepare record, the entries to apply to the person's Runtime config (remove Feld's skill, unchain `statusLine`, restore the replaced `otelHeadersHelper`, and so on) ([Claude Code](../spec/design/adapter/claude-code.md#preparing-for-attach), [Codex](../spec/design/adapter/codex.md#preparing-for-attach)). When a prepare record cannot be read, fail showing its path, without changing the Runtime config or the Feld home records
3. Decide what to remove from `$FELD_HOME`. By default, remove the settings for using Feld (`plugins/`, `prepared/`, the local token, Session tokens, self index, the Collector endpoint and settings, the daemon record and socket (for a Feld home whose path is too long for a socket, the one in `$TMPDIR/feld-<uid>/`), the log and its generations, spool, the `feld ui` page, locks), and keep `sessions/` and the store (`admin/feld.db`). `--purge` removes those too, and removes the emptied `$FELD_HOME`. Only names Feld placed are removed, so other files in `$FELD_HOME` remain
4. Combine 1 through 3 into a list and ask for consent. The preparation of a Runtime config root is shared by every `FELD_HOME`. When there is something to remove for a Runtime with no prepare record in `$FELD_HOME`, that Runtime's list shows a `warning:` line saying it was not placed by this home's `feld init`, that removing it also stops attach for other Feld homes, and that running `feld init` in that home restores it. On a TTY, it asks `[y/N]`; anything other than `y` removes nothing and ends with exit code 1. `--yes` counts as consent. When not on a TTY and without `--yes`, it shows ``run `feld uninstall --yes` to apply`` after the list and ends with exit code 1 without removing anything
5. With consent, kill the Sessions, wait for them to end, and stop the daemon. Reread the store and show Sessions left open (those that did not end while waiting after the kill, and those that started after the list) in `left open:` lines. Then call `restore` with `apply: true`, and finally remove from `$FELD_HOME`. `$FELD_HOME` comes last because the Adapter reads its records from `$FELD_HOME`
6. The `eval "$(feld env)"` in the shell config file and the `feld` and `feld-plugin-*` executables are shown in `next:` lines

Running `feld init` again reads the kept `sessions/` and store as they are under the same `FELD_HOME`.

```console
$ feld uninstall
Feld home: /Users/alice/.feld
daemon: running (pid 41230) · running sessions to kill: 1
  session_01j9y…  program      nightly-build

Claude Code
  skills/feld/         remove    /Users/alice/.claude/skills/feld
  otelHeadersHelper    restore   to: ~/.claude/otel-headers.sh
  statusLine           unchain   to: ~/.claude/statusline.sh

Codex
  skills/feld/         remove    /Users/alice/.codex/skills/feld
  hooks                remove    8 Feld hook entries
  writable_roots       remove    /Users/alice/.feld/spool/shared

Feld home
  plugins/ prepared/ admin/api-token tokens/ collector-endpoint admin/collector.json daemon.json daemon.log daemon.lock daemon.lifecycle.lock daemon.sock spool/ admin/ui-pair/   remove
  sessions/ admin/feld.db admin/feld.db-wal admin/feld.db-shm   keep   (use --purge to remove)

Remove these? [y/N] y
Uninstalled
  daemon:       stopped
  claude-code:  restored (/Users/alice/.claude/skills/feld, /Users/alice/.claude/settings.json)
  codex:        restored (/Users/alice/.codex/skills/feld, /Users/alice/.codex/hooks.json)
  feld home:    settings removed · sessions/ admin/feld.db admin/feld.db-wal admin/feld.db-shm kept at /Users/alice/.feld
  next:         remove `eval "$(feld env)"` from /Users/alice/.zprofile
  next:         remove /Users/alice/.local/bin/feld and the feld-plugin-* beside it
  note:         Codex keeps the trust of the Feld hooks under [hooks.state] of /Users/alice/.codex/config.toml; Codex manages it
```

`--json` returns the list, `applied` (whether anything was removed), and `next` in a single JSON object. Each Session has `kill` (whether it will be killed) and `killed` (whether it was killed), and each Runtime has `prepared` (whether `$FELD_HOME` has a prepare record for it).

## feld doctor

Check the environment Feld runs in and show each check's result on one line. It works even when the daemon is stopped or there is no Feld home (even before `feld init`), and writes nothing to the Feld home or the Runtime config. It does not inspect the Runtime's authentication state; it looks only at whether the executables exist and their versions.

```text
feld doctor
```

Each line shows the result (`ok` / `warn` / `fail`), the check name, and the detail, in that order, and `warn` and `fail` lines are followed by a `next:` line with the fix. `fail` means Feld's basic operations (starting the daemon, starting a Session, attach) are unavailable, and `warn` means some features are missing. On a TTY, results are colored ok (green), warn (yellow), and error (red).

```console
$ feld doctor
ok    executables       feld 0.2.0 · feld-plugin-claude-code, codex, tmux, herdr 0.2.0 · /Users/alice/.local/bin
ok    home              /Users/alice/.feld · runtimes: claude-code · providers: tmux
ok    store             schema 15
fail  daemon            not running · port 7381 is in use on 127.0.0.1
        next:  stop the process that holds port 7381 (`lsof -nP -iTCP:7381 -sTCP:LISTEN` names it), or run `feld daemon start --port <N>`
ok    provider          tmux 3.6a
ok    runtime           claude 2.1.289 (Claude Code)
ok    runtime-settings  claude-code: prepared
ok    sandbox           codex is not installed
warn  shell             eval "$(feld env)" is in /Users/alice/.zshrc, but this shell has not read it
        next:  source /Users/alice/.zshrc
ok    collector         http://127.0.0.1:4318 is free
ok    browser           open
```

| Check | What it looks at | `warn` | `fail` |
|---|---|---|---|
| `executables` | The bundled `feld-plugin-<name>` executables are in the same directory as `feld`, and `--version` ([plugin.md](../spec/design/plugin.md#invoking-an-operation)) reports the same version as `feld` | The executable of a Plugin that is not installed is missing | The executable of an installed Plugin is missing, there is no executable at all for a kind with no installed Plugin, the version differs from `feld`, or the version is not reported |
| `home` | `FELD_HOME` exists and is writable, at least one Runtime Adapter and one Execution Provider are installed, and the installed bundled Plugins have the same version as `feld` (`--version` is asked only when the contents differ from the executable next to `feld`) | — | `FELD_HOME` is missing or not writable, a kind has no installed Plugin, an installed bundled Plugin's version differs from `feld`, or the version is not reported (`next:` is `feld init`) |
| `store` | If the daemon answers status, the store is left to the daemon that opened it on startup and is not read. If the daemon is stopped, the store is opened read-only as immutable (without creating `-wal` and `-shm`) to see whether the schema is readable and its version. If there is no store, the daemon creates it on startup, and the daemon migrates older versions on startup | The process holding the daemon lock does not answer (not checked), or the daemon is stopped and `-wal` holds writes the daemon has not read back (reading would write `-shm`, so it is not checked; `next:` is `feld daemon start`) | Unreadable, or a version the daemon cannot migrate (`next:` is to move the store aside and start fresh) |
| `daemon` | Whether the daemon is running and its version matches `feld`. If stopped, whether the Port startup would use (the Port determined by steps 1–3 of [`feld daemon start`](#feld-daemon-start)) is free | Stopped (`next:` is `feld daemon start`) | The Port startup would use is in use, the daemon does not respond, or the daemon's version differs from `feld` (`next:` is `feld init`) |
| `provider` | The executables of the installed bundled Execution Providers (`tmux`, `herdr`) are on PATH and report a version | The version is not reported | The executable is not on PATH, or the version is older than the lower bound of the [supported versions](../spec/design/plugin.md#supported-versions) (`next:` is to install a version at or above the lower bound) |
| `runtime` | The executables of the installed bundled Runtime Adapters (`claude`, `codex`) are on PATH and report a version | The executable is not on PATH, the version is not reported, or the version is older than the lower bound of the [supported versions](../spec/design/plugin.md#supported-versions) (`next:` is to install a version at or above the lower bound) | — |
| `runtime-settings` | Calls `prepare` of the installed Runtime Adapters with `apply: false` to see whether the Runtime config root has `feld init`'s preparation (skill, hooks, and so on) complete and at the current version | Incomplete (`next:` is `feld init --runtime <name>`), or `prepare` does not answer | — |
| `sandbox` | When the Codex Runtime Adapter and a tmux or herdr Provider are installed, `sandbox_workspace_write.network_access` in Codex's `config.toml` ([the sandbox hole](../spec/design/spool.md#the-sandbox-hole)) | It is `true` (`next:` is to set it to `false`), or `config.toml` cannot be read | — |
| `shell` | Whether the shell config file has `eval "$(feld env)"` and the current shell's environment has the values of `feld env` | Missing from the config file, not read by the current shell (`next:` is `source <file>`), or a shell other than zsh and bash | — |
| `collector` | If the daemon is running, whether the Collector is receiving OTLP; if stopped, whether the Port of the Collector endpoint is free | The Collector is not running, or the endpoint's Port is in use | — |
| `browser` | Whether the means `feld ui` uses to open a browser (`BROWSER`, otherwise `open` on macOS and `xdg-open` elsewhere) can be run | It cannot be run (`next:` is `feld ui --print-url`) | — |

Non-bundled Plugins show only their name in `provider` and `runtime`. Checks that could not be verified are `warn`. When the Runtime's sandbox rejects TCP, `daemon` shows the pid and API of the daemon holding the lock from `daemon.json`, and together with `collector` is `warn`, with `feld doctor` outside the sandbox as `next:`. The spool is not used.

If any check is `fail`, the exit code is 1; otherwise 0. `--json` returns a `checks` array with each check's `name`, `result` (`ok` / `warn` / `fail`), `detail` (the detail), and `next` (the fix, or null if none).

## daemon

Run and inspect the daemon that holds the store and serves the API. `start` / `restart` handle the daemon process, so they do not call the API.

```text
feld daemon <COMMAND>
```

### feld daemon start

Start the daemon in the background and wait up to 10 seconds for the API to respond. The daemon restores Actors, open Sessions, and Schedules from the store, and re-observes each Session's underlying process through its Provider. If the daemon is already running, the command returns its state.

```text
feld daemon start [--port <PORT>] [--allow-host <HOST>]...
```

**Options**

- `--port <PORT>`: The API Port (environment variable `FELD_API_PORT`). Later starts use the same Port
- `--allow-host <HOST>`: The name of a proxy whose requests are accepted (a host name or IP address without Port, scheme, or path). Can be given multiple times

```console
$ feld daemon start
Daemon started.
  pid:        48213
  home:       /Users/alice/.feld
  api:        http://127.0.0.1:7381
  ui:         http://127.0.0.1:7381/ui/  ·  first time in this browser: run `feld ui` to sign in
  timezone:   Asia/Tokyo
  restored:   12 actors, 3 open sessions, 2 schedules
```

`ui:` shows the Web UI URL and that the first time in a browser, the person signs in with [`feld ui`](#feld-ui). It does not show a pairing code. `--json` returns the Web UI URL in `ui`. On a TTY, the heading is prefixed with `✓` and keys are dim. When started from [`feld init`](#feld-init), it collapses to the single line `Daemon:      started · pid 48213 · http://127.0.0.1:7381`.

The API listens on loopback `127.0.0.1` and, where IPv6 is available, `[::1]` on the same Port, so `localhost` reaches the daemon whichever it resolves to. The Port is determined in this order.

1. The Port given with `--port` (environment variable `FELD_API_PORT`). It is kept in `$FELD_HOME/daemon.json`, and later `start` / `restart` listen on the same Port
2. A Port previously given with `--port` and kept in `daemon.json`
3. In the default Feld home (`~/.feld`), the default Port `7381`. In other homes (tests, isolated daemons), the Port the previous daemon listened on, or, if it is taken, a Port free on both addresses chosen by the OS

When the Port from 1–2 or the default Port is in use on either address, startup ends with an error saying so, guiding the user to choose another Port with `--port`. `--port` while the daemon is running on a different Port is also an error, guiding the user to `feld daemon restart --port <N>`.

The daemon answers only requests sent to loopback names (`127.0.0.1`, `localhost`, `[::1]`, all on the daemon's Port) and to names allowed with `--allow-host`; for anything else, it returns 403 showing the command that allows it ([auth.md](../spec/design/auth.md#accepted-hosts)). The running daemon holds the allowed names, and [`restart`](#feld-daemon-restart) carries them over. Giving `--allow-host` a name the running daemon does not allow is an error, guiding the user to `feld daemon restart --allow-host <HOST>`.

The daemon log goes to `$FELD_HOME/daemon.log`. While running, when it reaches 10 MiB, the daemon moves it to `daemon.log.1`, shifts the existing generations up by one, and deletes those older than `daemon.log.4`. Every file is readable and writable only by the owner.

**Opening the Web UI from another device**

Exposure to other devices is left to a relay such as Tailscale. `tailscale serve` relays the daemon's Port to the tailnet.

```console
$ tailscale serve --bg 7381
$ feld daemon start --allow-host <host>.<tailnet>.ts.net
```

When a Port was chosen with `--port`, relay that Port. For `--allow-host`, give the name `tailscale serve status` shows. On the other device, open `/ui/` at `https://<host>.<tailnet>.ts.net/`, and on the Sign in screen enter the code that [`feld ui pair`](#feld-ui-pair) prints on the daemon's machine. Exposure stays within the tailnet.

### feld daemon status

Show the state of the running daemon.

```text
feld daemon status
```

It shows whether the daemon is running, its version (the same value as `feld --version`), pid, which store it serves, the API and Web UI, the names allowed with `--allow-host` (`(none)` if none), the Collector's OTLP endpoint, the local timezone used as the Schedule default, the installed Runtime Adapters and Execution Providers, Nodes, and the Scheduler state. The Scheduler state is the time it last woke, the number of restarts and the last reason, and the tick each Schedule is waiting for.

```console
$ feld daemon status
Daemon
  state:      running
  version:    0.2.0 (built 2026-10-01T00:00:00Z)
  pid:        48213
  home:       /Users/alice/.feld
  api:        http://127.0.0.1:7381
  web_ui:     http://127.0.0.1:7381/ui/
  allow_host: macbook.tail1234.ts.net
  otlp:       http://127.0.0.1:4318
  timezone:   Asia/Tokyo
  runtimes:   claude-code
  providers:  tmux, herdr
  nodes:      local
  scheduler:  woke 2026-09-13T11:43:00+09:00 · restarts 0
  ticks:      default/dispatcher 2026-09-13T12:13:00+09:00
```

Inside a sandbox that rejects TCP, it goes through the [spool](../spec/design/spool.md#choosing-the-path) and shows the same state. When the daemon is not running, it shows `Daemon is not running.` and the home and ends with exit code 1. When the process holding the daemon lock does not answer within 5 seconds, it shows `state: unresponsive`, the pid, home, API, and the next step, and ends with exit code 2. `--json` returns the same `state`.

```console
$ feld daemon status
Daemon
  state:      unresponsive
  pid:        48213
  home:       /Users/alice/.feld
  api:        http://127.0.0.1:7381
  next:       kill 48213 · feld daemon start
```

### feld daemon stop

Ask the daemon to exit. Sessions keep running on their Providers. When the daemon is not running, it prints `Daemon is not running.` with exit code 0. When the daemon does not answer within 5 seconds, it does not stop it, shows the pid and the remedy, and exits with code 2. `restart` behaves the same.

```text
feld daemon stop
```

```console
$ feld daemon stop
Daemon stopped. Sessions keep running on their providers.
```

### feld daemon restart

Stop the daemon and start it again so it reloads the installed Plugins. The new daemon carries over the local token and the stopped daemon's Port and allowed names, so open Web UIs and Sessions keep reading. A daemon in the default Feld home that listened on a Port other than the default moves to the default Port with `stop` → `start`.

```text
feld daemon restart [--port <PORT>] [--allow-host <HOST>]...
```

`--port` respecifies the Port as in [`start`](#feld-daemon-start), and `--allow-host` adds names to the carried-over allowed names. When the given Port is in use, the command ends with an error without stopping the running daemon.

```console
$ feld daemon restart
Daemon stopped. Sessions keep running on their providers.
Daemon started.
  pid:        48377
  home:       /Users/alice/.feld
  api:        http://127.0.0.1:7381
  ui:         http://127.0.0.1:7381/ui/  ·  first time in this browser: run `feld ui` to sign in
  timezone:   Asia/Tokyo
  restored:   12 actors, 3 open sessions, 2 schedules
```

## workspace

Group Actors into Workspaces. Actor names are unique per Workspace.

```text
feld workspace <COMMAND>
```

### feld workspace list

List Workspaces with their Actor counts.

```text
feld workspace list [OPTIONS]
```

**Options**

- `--include-deleted`: Also list deleted Workspaces, adding a `DELETED` column

```console
$ feld workspace list --include-deleted
KEY       ACTORS  CREATED                    DELETED
default   9       2026-09-01T09:12:40+09:00  -
research  0       2026-09-08T14:03:11+09:00  2026-09-20T18:02:44+09:00
```

### feld workspace create

Create a Workspace.

```text
feld workspace create <KEY>
```

**Arguments**

- `<KEY>`: Workspace key. Consists of lowercase letters, digits, and `-`, and starts with a letter or digit

```console
$ feld workspace create research
Workspace created.
  key:        research
  created_at: 2026-09-08T14:03:11+09:00
```

### feld workspace delete

Soft delete a Workspace other than default that has no live Actors. A new Workspace can then be created with that key.

```text
feld workspace delete <KEY>
```

**Arguments**

- `<KEY>`: The key of the live Workspace to delete

```console
$ feld workspace delete research
Workspace deleted.
  key:        research
  deleted_at: 2026-09-20T18:02:44+09:00
```

### feld workspace get

Show a Workspace. For a deleted key, it shows the most recently deleted one.

```text
feld workspace get <KEY>
```

**Arguments**

- `<KEY>`: Workspace key

```console
$ feld workspace get research
Workspace
  key:        research
  actors:     3
  created_at: 2026-09-08T14:03:11+09:00
  deleted_at: -
```

## actor

Creates, changes, and reads the Actors of a Workspace.

```text
feld actor <COMMAND>
```

The start in `create`, `start`, and the next generation of `restart` follow [admission](../spec/concepts/glossary.md#observation) by the latest observation of the current Session.

Commands that call several APIs in sequence write the declaration first; if a step fails, they stop and show in the output the stages that were committed. Running the same command again continues from there. The declaration remains even when the start is rejected.

When the Session never started, the command shows what was committed as is, adds `start failed` to `result`, puts the Provider's observation detail in `reason`, and ends with exit code 1.

```console
$ feld actor create smoke --kind job --workdir ~/Dev/feld-issue-12 --start -- bash scripts/smoke.sh
Actor declaration committed; Session failed to start.
  actor:        smoke (job)
  command:      bash scripts/smoke.sh
  workdir:      ~/Dev/feld-issue-12
  provider:     tmux · local
  session:      session_3c7b21
  generation:   1
  start_kind:   start
  placement:    tmux · local · session pending
  result:       unknown (start failed)
  reason:       invalid tmux provider request: prepared workdir must still be an existing absolute directory
```

### feld actor create

Declares an Actor and records its launchSpec. An `agent` runs a Runtime; a `job` runs a program. An agent starts a Session immediately; a job waits. The creation is committed first, so a later start failure ends with `Actor declaration committed; <reason>` and exit code 1.

```text
feld actor create [OPTIONS] --kind <KIND> <NAME> [-- <COMMAND>...]
```

**Arguments**

- `<NAME>`: Actor name. Unique among the live Actors of the Workspace
- `[COMMAND]...`: The program's argv, given after `--`

**Options**

- `--kind <KIND>`: `agent` or `job` (required). Fixed for the Actor's lifetime
- `--cron <CRON>`: 5-field cron expression on which the Job's Schedule fires, such as `'0 9 * * *'`
- `--timezone <TIMEZONE>`: IANA timezone in which the Schedule reads the cron. Requires `--cron`. Defaults to the daemon's timezone
- `--start`: Start a Session after the creation is committed. Default for agent
- `--no-start`: Do not start a Session. Default for job. Cannot be combined with `--start`
- `--grant <GRANT>`: [grant](../spec/concepts/glossary.md#grants) that allows the Actor's Sessions to operate on other Actors in the Workspace: `read`, `control`, `manage-agent`, `manage-job`. Can be given multiple times
- `--accept-from <ACTOR>`: Workspace Actor from which, besides Admin, the Actor accepts Messages. Can be given multiple times. Defaults to every Actor in the Workspace
- [launchSpec options](#launchspec-options)

An agent takes `--runtime` and a job takes `-- <COMMAND>...`; any other combination is rejected without writing the declaration. Only a Job accepts `--cron`, which declares a Schedule. Feld starts a Job with a Schedule from the next tick. Without `--timezone`, the daemon's local timezone is resolved at creation and stored.

If a live Actor with the same name exists, its declaration is replaced entirely under the same ID. The launchSpec becomes the given values; the Schedule is replaced if `--cron` is given (keeping `enabled`) and removed otherwise. A different kind is rejected. Only Admin grants grants, and `--grant` from inside a Session is rejected. Without `--grant`, the replacing declaration keeps the grants. `--accept-from` gives [acceptFrom](../spec/concepts/glossary.md#acceptfrom); without it, the replacing declaration keeps the acceptFrom.

When a `--runtime codex` Actor runs a `workspace-write` sandbox with network allowed on tmux or herdr, a `note:` line shows the risk and the fix, and when starting a Session, it is shown on stderr before the start is requested ([sandbox hole](../spec/design/spool.md#the-sandbox-hole)). The same applies to `actor update`.

```console
$ feld actor create dev-issue-15 --kind agent --runtime claude-code --workdir ~/Dev/feld-issue-15
Actor created and session started.
  actor:        dev-issue-15 (agent)
  runtime:      claude-code
  workdir:      ~/Dev/feld-issue-15
  provider:     tmux · local
  session:      session_9c41e2
  generation:   1
  start_kind:   start
  placement:    tmux · local · window @7 (main:1, created)
  result:       running (start settled 2026-09-13T10:42:09+09:00)
```

### feld actor update

Changes an Actor's name, launchSpec, Schedule, grants, and acceptFrom. Only the given items change. `--name` renames the Actor, keeps its `id`, Session history, and mailbox, and shows the old and new names in the output. Renaming to the name of another live Actor and renaming an Actor whose current Session is running are rejected. The launchSpec and Schedule leave the current Session untouched and take effect from the next start or tick. Grants and acceptFrom take effect from the decision on the next request. At least one option is required.

```text
feld actor update [OPTIONS] <ACTOR> [-- <COMMAND>...]
```

**Arguments**

- `<ACTOR>`: Actor to update
- `[COMMAND]...`: The program's argv, given after `--`

**Options**

- `--name <NAME>`: New name of the Actor
- `--cron <CRON>`: 5-field cron expression of the Schedule. Creates a Schedule for a Job without one
- `--timezone <TIMEZONE>`: IANA timezone in which the Schedule reads the cron
- `--grant <GRANT>`: grant that replaces all of the Actor's grants: `read`, `control`, `manage-agent`, `manage-job`. Can be given multiple times
- `--no-grants`: Remove all of the Actor's grants. Cannot be combined with `--grant`
- `--accept-from <ACTOR>`: Replace all Actors from which Messages are accepted. Can be given multiple times
- `--no-accept-from`: Return to accepting Messages from every Actor in the Workspace
- [launchSpec options](#launchspec-options)

An agent keeps its Runtime and a job keeps its program; an update that gives the other is rejected. Giving any Provider option replaces `providerOptions` entirely, and `--env` replaces `env` entirely. The Schedule keeps `enabled`, and keeps its timezone when `--timezone` is omitted. As with `create`, `--grant` and `--no-grants` are rejected from inside a Session. `--accept-from` replaces acceptFrom entirely with the given Actors, and `--no-accept-from` returns it to omitted.

```console
$ feld actor update dispatcher --cron "*/20 * * * *"
Actor updated.
  actor:        dispatcher (job)
  schedule:     */20 * * * * (Asia/Tokyo), enabled
  next_run:     2026-09-13T12:20:00+09:00
  applies:      next tick (session_41ac09 untouched)

$ feld actor update reviewer --name reviewer-14
Actor updated.
  actor:        reviewer-14 (agent)
  renamed:      reviewer -> reviewer-14
```

### feld actor delete

Soft-deletes an Actor, keeping its history. Rejected if the current Session is running. The name is immediately available for a new Actor, and the deleted Actor can be read with `feld actor get <ID>` and `feld actor list -a --include-deleted`.

```text
feld actor delete <ACTOR>
```

**Arguments**

- `<ACTOR>`: Actor name in the Workspace

```console
$ feld actor delete dev-issue-14
Actor deleted.
  actor:        dev-issue-14 (agent)
  id:           actor_51d0a8
  deleted_at:   2026-09-13T12:31:40+09:00
```

### feld actor get

Shows an Actor's ID, launchSpec, Schedule, and Sessions. A name reads a live Actor; an ID reads any Actor, including deleted ones. Sessions are listed newest first, and the current Session is marked with `*`.

```text
feld actor get <ACTOR>
```

**Arguments**

- `<ACTOR>`: Actor name or Actor ID (`actor_...`)

The fields are the ID, kind, launchSpec (`env` shows only variable names; `--json` shows the values), grants, acceptFrom (`*` if omitted), and, for a Job, the Schedule, next run, number of skipped ticks, and the overdue tick if any. The Session history shows only recent Sessions. Read all Sessions with `feld session list --actor <name>`, the details of one Session with `feld session get`, and skipped ticks with `feld record list --actor <name> --kind schedule.skipped`.

```console
$ feld actor get dispatcher
Actor
  name:             dispatcher
  id:               actor_0c93f4
  kind:             job
  workspace:        default
  launch_spec:      python3 dispatch.py · ~/Dev/my-project · tmux · local
  env:              GH_CONFIG_DIR, GIT_AUTHOR_NAME
  schedule:         13,43 * * * * (Asia/Tokyo) · enabled since 2026-09-10T09:00:00+09:00
  next_run:         2026-09-13T12:13:00+09:00
  skipped_ticks:    1
  grants:           read,control
  accept_from:      *
  deleted_at:       -
Sessions (38)
   GEN  SESSION         START                      START_KIND  OUTCOME
*  38   session_41ac09  2026-09-13T11:43:00+09:00  schedule    running
   37   session_3b8e0f  2026-09-13T11:13:00+09:00  schedule    succeeded (exit_code 0)
   36   session_1f6c22  2026-09-13T10:43:00+09:00  schedule    succeeded (exit_code 0)
```

### feld actor list

Lists the Workspace's Actors with their latest Session. By default it lists only Actors with a current Session and Jobs with an enabled Schedule, and shows the number of unlisted Actors in a line `<N> stopped Actors not shown (use -a)` after the table (`hiddenCount` in `--json`).

```text
feld actor list [OPTIONS]
```

**Options**

- `-a`, `--all`: List all Actors
- `--kind <KIND>`: Limit to `agent` or `job` Actors
- `--refresh`: Observe each current Session through its Execution Provider before showing
- `--include-deleted`: With `-a`, also list deleted Actors and add a `DELETED` column

Each row has the kind; the generation, start kind, outcome, and observation of the current Session, or of the latest Session if there is none; the number of unread Messages (`WAITING`); marks (`mail`, `skip`, `overdue`, `unlinked`); the next run time; grants; and acceptFrom (`*` if omitted, `-` if empty). The order is the same as the Web UI bands (current → attention → others). In `--json`, the current Session of an agent-runtime Session carries the [turn](../spec/concepts/glossary.md#turn) and the last activity time (`turn`, `lastEntryAt`).

```console
$ feld actor list
Workspace: default
NAME             KIND   SESSION         GEN  START        OUTCOME    OBSERVED  WAITING  MARKS  NEXT_RUN                   GRANTS        ACCEPT_FROM
dev-issue-15     agent  session_9c41e2  1    start        running    running   0        -      -                          -             *
my-project-1     agent  session_2d8b6e  3    attach       running    running   0        -      -                          -             *
dispatcher       job    session_41ac09  38   schedule     running    running   1        mail   2026-09-13T12:13:00+09:00  read,control  *
dev-issue-13     agent  session_5f7a28  1    start        running    lost      0        -      -                          -             *
3 stopped Actors not shown (use -a)

$ feld actor list -a
Workspace: default
NAME             KIND   SESSION         GEN  START        OUTCOME    OBSERVED  WAITING  MARKS  NEXT_RUN                   GRANTS        ACCEPT_FROM
dev-issue-15     agent  session_9c41e2  1    start        running    running   0        -      -                          -             *
my-project-1     agent  session_2d8b6e  3    attach       running    running   0        -      -                          -             *
dispatcher       job    session_41ac09  38   schedule     running    running   1        mail   2026-09-13T12:13:00+09:00  read,control  *
dev-issue-13     agent  session_5f7a28  1    start        running    lost      0        -      -                          -             *
dev-issue-14     agent  session_77f0be  1    start        exited     gone      0        -      -                          -             *
pr-checks        job    session_c3e5d1  2    replacement  succeeded  gone      2        -      -                          -             *
nightly          job    -               -    -            -          -         0        -      paused                     -             *
```

### feld actor start

Starts a Session from the launchSpec. The first generation gets `startKind: start`; if a previous Session exists, it becomes the next generation (`startKind: replacement`) and replaces the ended current Session. If the current Session is running, the command rejects with exit code 1.

```text
feld actor start <ACTOR>
```

**Arguments**

- `<ACTOR>`: Actor name in the Workspace

This command also runs a Job with a Schedule without waiting for the tick, and starts an Agent created with `--no-start` or a Job without a Schedule. An Actor without a launchSpec is rejected. [spawned](../spec/concepts/glossary.md#spawned) is recorded; `spawner` is the calling Actor when called from inside a Session, and `Admin` otherwise.

```console
$ feld actor start reviewer-01
Start accepted and committed.
  actor:        reviewer-01 (agent)
  session:      session_e12f77 (replaces session_5a90c3)
  generation:   2
  start_kind:   replacement
  placement:    tmux · local · window @9 (main:2, created)
  result:       running (start settled 2026-09-13T11:02:41+09:00)
```

### feld actor attach

Makes a Runtime running in a pane the Session of an Actor. Creates an Actor of kind `agent` and an `attached` Session (`startKind: attach`), starting nothing and sending no signal. Requires either `--self` or a Provider option that points to a pane.

```text
feld actor attach [OPTIONS] --runtime <RUNTIME> [ACTOR]
```

**Arguments**

- `[ACTOR]`: Actor name to attach. When omitted with `--self`, it is the Actor whose current Session holds the same Runtime process, or otherwise a name derived from the pane's workdir

**Options**

- `--self`: Attach the Runtime of the pane in which this CLI runs. When the Runtime runs its Runtime-side session in a process outside the pane (Codex's managed app-server), add the pane with a Provider option. Without it, the command shows the candidate panes and a retry command pointing to each, and ends with exit code 1
- `--runtime <RUNTIME>`: Runtime running in the pane: `claude-code` or `codex` (required)
- `--tmux-pane <ID>`: tmux pane to attach
- `--tmux-socket <NAME|PATH>`: Socket of that tmux. Defaults to the socket in `TMUX`, or `feld` outside tmux
- `--herdr-pane <ID>`: herdr pane to attach
- `--herdr-session <NAME|PATH>`: Server of that herdr. Defaults to `HERDR_SOCKET_PATH`, or `default` outside herdr

There are two forms.

- `feld actor attach <ACTOR> --runtime <RUNTIME> --<provider>-<key> <VALUE>...`: Attaches by pointing from outside at an existing pane with the Runtime in the foreground. `<ACTOR>` is required. The Provider defines the form of the target and the resolution rules ([tmux](../spec/design/provider/tmux.md#--tmux-pane), [herdr](../spec/design/provider/herdr.md#--herdr-pane))
- `feld actor attach [<ACTOR>] --runtime <RUNTIME> --self`: Registers the Runtime itself from inside. The Provider builds the Placement from the caller's environment ([tmux](../spec/design/provider/tmux.md#--self), [herdr](../spec/design/provider/herdr.md#--self)). This is the form the Runtime Adapter's skill (Claude Code's `/feld attach`, Codex's `feld` skill) calls. For a Runtime that runs outside the pane ([Codex](../spec/design/adapter/codex.md#the-attach-flow)), a person chooses the pane ([plugin.md](../spec/design/plugin.md#division-of-work-on-attach)). Attaching to an existing Actor with `<ACTOR>` carries over its mailbox and history. The default name is `<workdir basename>-N`. The base is the basename of the pane's workdir with ASCII uppercase lowered, each run of characters other than `[a-z0-9]` replaced by a single `-`, and leading and trailing `-` removed (`agent` if empty). N is the first positive integer from 1 for which no Actor of that name exists, or the Actor's kind is `agent` and passes admission; for an existing Actor, the next generation is added. If the attach to the chosen name is rejected, it retries with the next N

The Session's execution is determined by `--runtime` and fixed at attach time. The Actor's launchSpec stays as is, and whether it can `start` depends on whether the Actor has a launchSpec. The Session closes when the attached Runtime process ends.

- When attaching to an existing Actor, the command rejects if its current Session is running, and rejects a Job. However, an attach to the same Runtime process as an `attached` current Session (matching pid and start time) reissues that Session's token without creating a new generation, and the old token becomes invalid
- If an Actor holds that Runtime process (for an `owned` Session, the program's process) as its current Session, the command uses that Actor's name when `<ACTOR>` is omitted, and rejects when a different name is given
- `--self` from a call whose environment has `FELD_SESSION_TOKEN` (inside an `owned` Session) is rejected
- A Runtime or Provider that is not installed is rejected, and if no plugin of that kind is installed, the command points to the `feld init` that places the default (e.g. `feld init --runtime claude-code`)
- The CLI puts the Session token from the response in the self index ([auth.md](../spec/design/auth.md#attached-sessions)) so that `feld` inside the pane can speak as that Session
- Before the attach, the Runtime Adapter of `--runtime` checks whether the Runtime preparation done by [`feld init`](#feld-init) is present. Without it (including after [`feld uninstall`](#feld-uninstall)), the attach is still accepted and a `note:` line points to `feld init`
- After the attach, the CLI delivers the Runtime identity answered by the Runtime Adapter's `identity` through `POST /v1/self/runtime` of the [Runtime API](../spec/concepts/glossary.md#runtime-api), and starts the components that run beside the Runtime through the Runtime Adapter's `attached`. With `--self`, the identity comes from the caller's environment; for a pane pointed at from outside, from the foreground Runtime process. When the identity cannot be delivered or the components cannot start, a `note:` line says so ([Claude Code](../spec/design/adapter/claude-code.md#the-attach-flow), [Codex](../spec/design/adapter/codex.md#the-attach-flow))
- For a run whose Runtime telemetry cannot be attributed to the Session, the output's `telemetry` line shows that the Session's metrics are `unavailable`, with the reason and the condition for delivery answered by the Runtime Adapter. For Claude Code, the condition is restarting from a shell that loads [`feld env`](#feld-env)

```console
$ feld actor attach codex-dev --runtime codex --tmux-pane %3
Attach accepted and committed. Nothing was started or signalled.
  actor:        codex-dev (agent)
  session:      session_b07d3a
  start_kind:   attach (attached)
  execution:    agent-runtime · codex
  placement:    tmux · local · pane %3 (main:1, existing)
  observation:  running (2026-09-13T10:45:02+09:00) · the attached Runtime incarnation is running
```

### feld actor restart

Requests the running current Session to stop in the same way as [`feld session stop`](#feld-session-stop), and after the Execution Provider observes the end, starts the next generation (`startKind: replacement`) from the launchSpec. Actors whose current Session is not running and Actors whose current Session is attached are rejected. The next generation records [spawned](../spec/concepts/glossary.md#spawned). If the next generation does not appear within 30 seconds, the command ends showing the Session waiting to stop on a `stopping:` line, and the next generation starts after that Session closes.

```text
feld actor restart <ACTOR>
```

**Arguments**

- `<ACTOR>`: Actor name in the Workspace

```console
$ feld actor restart developer-01
Restart accepted and committed.
  actor:        developer-01 (agent)
  stopped:      session_a3d4f0 (stopped, exit_code 0)
  session:      session_b81c55 (replaces session_a3d4f0)
  generation:   3
  start_kind:   replacement
  placement:    tmux · local · window @11 (main:3, created)
  result:       running (start settled 2026-09-13T12:40:17+09:00)
```

### feld actor pause

Sets `enabled` of a Job's Schedule to `false` to stop its ticks. No tick fires until `feld actor resume`, and the running Session is left untouched. A Job without a Schedule is rejected with ``Actor '<job>' has no Schedule; add one with `feld actor update <job> --cron ...` ``.

```text
feld actor pause <ACTOR>
```

**Arguments**

- `<ACTOR>`: Actor name in the Workspace

```console
$ feld actor pause dispatcher
Schedule paused.
  enabled:      no (since 2026-09-13T12:05:31+09:00)
  current:      session_41ac09 (running, not touched)
```

### feld actor resume

Sets `enabled` of a Job's Schedule to `true`. The next run time is computed from now, and ticks missed while paused are not caught up. Otherwise the same as `pause`.

```text
feld actor resume <ACTOR>
```

**Arguments**

- `<ACTOR>`: Actor name in the Workspace

```console
$ feld actor resume dispatcher
Schedule resumed.
  enabled:      yes (since 2026-09-13T14:12:08+09:00)
  next_run:     2026-09-13T14:20:00+09:00
```

## session

Reads and operates on Sessions, the runs of an Actor.

```text
feld session <COMMAND>
```

### feld session get

Shows a Session. Ended Sessions can also be read. Besides start, end, outcome, execution, placement, and the last observation, it shows the spawner, the requester and result of accepted stop / kill requests, and the Sub Agent Sessions the Runtime reported. Given a Sub Agent Session ID, it shows that Sub Agent Session's parent Session, Sub Agent, start / end, outcome, execution, and model.

```text
feld session get <TARGET>
```

**Arguments**

- `<TARGET>`: Session ID, Sub Agent Session ID (as shown by `feld session get`), or Actor name. An Actor name points to the current Session, or to the latest Session if there is none

The `observation` line adds the Provider's `detail` to the kind and time, and `start_settled` of a Session that never started shows `- · start failed`. A Session whose `execution` is `agent-runtime` also shows `model`, and a Session whose `startKind` is `start` / `replacement` also shows the spawned `spawner` (`spawned_by`). For a Session from which the Provider can derive an attach command, that is shown too. For a Session with the [unlinked mark](../spec/concepts/glossary.md#unlinked-mark), `model` shows `- · unlinked`, and a `note` line points to responding in the Runtime's pane. The `--json` of a current agent-runtime Session carries the [turn](../spec/concepts/glossary.md#turn) and the last activity time (`turn`, `lastEntryAt`).

```console
$ feld session get dev-issue-15
Session session_9c41e2
  actor:            dev-issue-15 (agent)
  generation:       1
  start:            2026-09-13T10:42:07+09:00
  end:              -
  start_kind:       start (owned) · spawned_by dispatcher (session_0aa4d9)
  end_kind:         running
  outcome:          running
  execution:        agent-runtime · claude-code
  model:            claude-fable-5-1
  placement:        tmux · local · window @7 (main:1, created) · ~/Dev/feld-issue-15
  start_settled:    2026-09-13T10:42:09+09:00
  observation:      running (2026-09-13T11:50:00+09:00) · the recorded tmux incarnation is running
  attach_command:   tmux -L feld attach-session -t '@7'
```

### feld session list

Lists Sessions newest first. `--start` / `--end` keep Sessions that overlap the range.

```text
feld session list [OPTIONS]
```

**Options**

- `--actor <ACTOR>`: Limit to this Actor's Sessions
- `--kind <KIND>`: Limit to Sessions of `agent` or `job` Actors
- `--current`: Limit to current Sessions (those without an end)
- `--start <TIME>`: Limit to Sessions that end after this time
- `--end <TIME>`: Limit to Sessions that start before this time

```console
$ feld session list --actor reviewer-01
Workspace: default
SESSION         ACTOR        GEN  START                      START_KIND   END_KIND  OUTCOME
session_e12f77  reviewer-01  2    2026-09-13T11:02:39+09:00  replacement  running   running
session_5a90c3  reviewer-01  1    2026-09-13T10:40:18+09:00  start        stopped   stopped (exit_code 137)
```

### feld session stop

Requests a Session to end in the Runtime's own way. The Execution Provider sends the Runtime Adapter's stop input (`/exit` for Claude Code, `/quit` for Codex) to the pane and observes after each send. An execution without a stop input (including a program) gets SIGTERM. Attached Sessions are rejected.

```text
feld session stop <TARGET>
```

**Arguments**

- `<TARGET>`: Session ID or Actor name (current Session)

The acceptance of the request is recorded in `lifecycle.stop`, and the Session closes with `endKind: stopped` when the Provider observes the end. Until the end is observed, the Session keeps running and holds `lifecycle.stop`. When the Provider could not answer, the output shows `attempt: unknown` and `observed: unknown`, and the Session stays open. Running the command again retries, with the heading `Lifecycle action was already requested.`

```console
$ feld session stop reviewer-01
Stop accepted and committed.
  session:      session_e12f77
  accepted:     2026-09-13T11:30:12+09:00
  attempt:      tmux sent /exit to the pane
  observed:     gone (2026-09-13T11:30:15+09:00)
  result:       stopped (exit_code 0)
```

### feld session kill

Ends a Session immediately with the Execution Provider's SIGKILL. The Runtime can neither finish its turn nor write its state. Attached Sessions are rejected.

```text
feld session kill <TARGET>
```

**Arguments**

- `<TARGET>`: Session ID or Actor name (current Session)

Can be run even after `stop` was accepted, as long as there is no `end`. The acceptance of the request is recorded in `lifecycle.kill`; otherwise the same as `stop`. `stop` / `kill` on a `lost` / `mismatched` Session is accepted, records `attempt: nothing to kill` and that kind in `observed`, and closes the Session with `endKind: unknown`.

```console
$ feld session kill reviewer-01
Kill accepted and committed.
  session:      session_5a90c3
  accepted:     2026-09-13T10:58:12+09:00
  attempt:      tmux killed the pane's process (SIGKILL)
  observed:     gone (2026-09-13T10:58:12+09:00)
  result:       stopped (exit_code 137)
```

### feld session observe

Has the Execution Provider observe the Session now and commits the result. Shows the observation and the previous observation. If the kind changed from the previous one, an `observation` record is also kept. Closed Sessions are rejected.

```text
feld session observe <TARGET>
```

**Arguments**

- `<TARGET>`: Session ID or Actor name (current Session)

```console
$ feld session observe dev-issue-13
Observation committed.
  session:      session_5f7a28
  observation:  lost (2026-09-13T12:02:17+09:00) · the tmux server incarnation is no longer running
  previous:     running (2026-09-13T11:50:00+09:00)
```

### feld session log

Reads a Session's output: the transcript for an agent-runtime Session, and stdout / stderr lines for a program Session. Given a Sub Agent Session ID, it reads that Sub Agent's transcript entries. The trailing `next cursor:` is the value for the next `--after`.

```text
feld session log [OPTIONS] <TARGET>
```

**Arguments**

- `<TARGET>`: Session ID, Sub Agent Session ID (as shown by `feld session get`), or Actor name. An Actor name points to the current Session, or to the latest Session if there is none

**Options**

- `--follow`: Keep printing additions until the Session ends. With `--json`, prints one entry or line per line
- `--tail <N>`: Print only the last N items. Up to 1000. Cannot be combined with `--after`
- `--after <CURSOR>`: Print only items after this cursor
- `--view <VIEW>`: Transcript entries to print: `conversation` (default) or `all`. Meaningful only for agent-runtime Sessions

The shape of `--json` depends on the kind of Session. Each line of `--follow --json` has the same shape as an element of `records` or `lines` in `--json`, and its `cursor` or `sequence` is the cursor for reading on.

```console
$ feld session log dispatcher --tail 4
Output session_41ac09 (program · python3 dispatch.py)
2026-09-13T11:43:01+09:00  stdout  ready issues: 2 (#15, #16)
2026-09-13T11:43:02+09:00  stdout  running developers: 1 / 2
2026-09-13T11:43:05+09:00  stdout  started dev-issue-16 (session_c2e871)
2026-09-13T11:43:06+09:00  stderr  warning: issue #16 has no Priority
next cursor: 42
```

### feld session metrics

Shows Session metrics, one line per metric. Each line has the status, source, and value over the whole range. `--json` adds a series in bins of `--bucket` seconds. Given a Sub Agent Session ID, it shows every metric as `n/a`. Metrics and aggregation rules follow [metrics.md](../spec/design/metrics.md).

```text
feld session metrics [OPTIONS] <TARGET>
```

**Arguments**

- `<TARGET>`: Session ID, Sub Agent Session ID (as shown by `feld session get`), or Actor name. An Actor name points to the current Session, or to the latest Session if there is none

**Options**

- `--start <TIME>`: Start of the range. Defaults to the Session's start
- `--end <TIME>`: End of the range. Defaults to the Session's end, or now if there is none
- `--bucket <SECONDS>`: Bin width of the `--json` series. 60 or more. Requires `--json`

```console
$ feld session metrics dev-issue-15
Metrics session_9c41e2 (Claude Code · claude-fable-5-1)
METRIC                   STATUS    SOURCE                         VALUE
token usage              reported  Claude Code Runtime telemetry  412380 total
estimated cost           reported  Claude Code Runtime telemetry  $4.12
API errors               reported  Claude Code Runtime telemetry  0
prompt cache hit rate    reported  Claude Code Runtime telemetry  91%
API request latency p95  reported  Claude Code Runtime telemetry  8420 ms
tool calls               reported  Claude Code Runtime telemetry  126
context window           reported  Claude Code status line        38% of 1000000
active time              reported  Claude Code Runtime telemetry  41m 12s
```

## record

Reads the facts Feld itself has recorded.

```text
feld record <COMMAND>
```

### feld record list

Lists Feld [records](../spec/concepts/glossary.md#record) in commit order. Shows Session starts and ends, spawns, observations, Messages, lifecycle requests, and skipped or missed ticks oldest first with their `SEQ`; the trailing `next cursor:` is the value for the next `--after`.

```text
feld record list [OPTIONS]
```

**Options**

- `--actor <ACTOR>`: Limit to the records of this Actor
- `--session <SESSION>`: Limit to the records of this Session (`session_...`)
- `--kind <KIND>`: Limit to records of this kind, such as `spawned` or `schedule.skipped`
- `--start <TIME>`: Limit to records at or after this time
- `--end <TIME>`: Limit to records before this time
- `--after <SEQ>`: Output only records after this sequence number
- `--follow`: Keep outputting newly committed records. With `--json`, outputs one record per line

`observation` adds the Provider's `detail` to the kind, and `message.accepted` adds `waiting` when unread. Each line of `--follow --json` has the same shape as an element of `records` in `--json`, and its `sequence` is the cursor for reading on.

```console
$ feld record list --session session_9c41e2
Records (7)
SEQ  AT                         ACTOR         KIND              DETAIL
1    2026-09-13T10:42:07+09:00  dev-issue-15  spawned           by dispatcher (session_0aa4d9)
2    2026-09-13T10:42:07+09:00  dev-issue-15  session.started   start_kind=start
3    2026-09-13T10:42:09+09:00  dev-issue-15  observation       running · the recorded tmux incarnation is running
4    2026-09-13T10:42:11+09:00  dev-issue-15  message.accepted  message_a41b7d from dispatcher (session_0aa4d9)
5    2026-09-13T10:42:30+09:00  dev-issue-15  message.read      message_a41b7d
6    2026-09-13T11:12:52+09:00  dev-issue-15  message.sent      message_c90e12 to dispatcher
7    2026-09-13T11:45:03+09:00  dev-issue-15  message.accepted  message_f2a9d0 from Admin (waiting)
next cursor: 7
```

## message

Sends, deletes, reads, and lists Messages between Actors. Alias `msg`.

```text
feld message <COMMAND>
```

A Message is addressed to an Actor, not to a Session.

### feld message send

Sends a Message to an Actor's mailbox. Feld accepts and stores it, and the Message stays `waiting` until a Session of that Actor reads it. The sender is the Actor of the resolved Session token, or Admin if there is none. From inside a Session, a Message can be sent only to Actors in the same Workspace whose [acceptFrom](../spec/concepts/glossary.md#acceptfrom) accepts the sender's Actor.

```text
feld message send [OPTIONS] <ACTOR> [BODY]
```

**Arguments**

- `<ACTOR>`: The destination Actor
- `[BODY]`: The body. If omitted, standard input becomes the body. A whitespace-only body exits with exit code 1

**Options**

- `--label <LABEL>`: Attach a [label](../spec/concepts/glossary.md#message) indicating the kind of Message. Exit code 2 if it does not match the same grammar as Actor names (`^[a-z0-9][a-z0-9-]*$`)

```console
$ feld message send dev-issue-15 "Please address the review comments"
Message accepted and committed.
  message:      message_f2a9d0
  from:         Admin
  to:           dev-issue-15
  accepted:     2026-09-13T11:45:03+09:00
  state:        waiting
  anchor:       feld:message_f2a9d0
```

### feld message delete

Deletes a Message that no Session has read. Rejects Messages that have a read and Messages in a Runtime's queue.

```text
feld message delete <MESSAGE>
```

**Arguments**

- `<MESSAGE>`: Message ID

```console
$ feld message delete message_f2a9d0
Message deleted.
  message:      message_f2a9d0
  to:           dev-issue-15
  deleted_at:   2026-09-13T11:46:20+09:00
```

### feld message get

Shows a Message's route, label (only when present), read state, and full body. It records nothing, so a `waiting` Message stays `waiting`.

```text
feld message get <MESSAGE>
```

**Arguments**

- `<MESSAGE>`: Message ID

A deleted Message shows `read` as `- (deleted)` and adds `deleted_at`. A Message in a Runtime's queue shows `read` as `- (queued <time> by <Session>)`. An Adapter with a limit on notification length delivers long bodies as excerpts, and the Agent reads the full body with this command.

```console
$ feld message get message_e88a21
Message
  message:      message_e88a21
  from:         dev-issue-15 (session_9c41e2)
  to:           dispatcher
  label:        report
  accepted:     2026-09-13T11:58:44+09:00
  read:         - (waiting)
  anchor:       feld:message_e88a21
  body:
    Addressed the 3 comments and CI passed. Please re-check PR #62.
```

### feld message list

Lists Messages addressed to Actors of the current Workspace, oldest first. It does not mark them read. Each row has the sender, destination, label (`-` if none), accept time, read state, and preview; filtered with `--to`, the list is that Actor's mailbox.

```text
feld message list [OPTIONS]
```

**Options**

- `--from <ACTOR>`: Limit to Messages sent by this Actor
- `--to <ACTOR>`: Limit to Messages addressed to this Actor
- `--label <LABEL>`: Limit to Messages with this label
- `--waiting`: Limit to Messages without a read
- `--start <TIME>`: Limit to Messages accepted at or after this time
- `--end <TIME>`: Limit to Messages accepted before this time

The read state is one of `waiting`, `queued`, or `read`. `queued` is a Message in a Runtime's queue awaiting a read, and it counts toward `waiting`. A line before the table states that `read` means handoff to a Session.

```console
$ feld message list --to dispatcher
Messages (3, 1 waiting)
A read means the Message was handed to a Session, not that it was acted on.
MESSAGE          FROM          TO          LABEL   ACCEPTED                   STATE    PREVIEW
message_c90e12   dev-issue-15  dispatcher  report  2026-09-13T11:12:52+09:00  read     Created PR #62. CI is…
message_d17b03   dev-issue-14  dispatcher  -       2026-09-13T11:20:08+09:00  read     Could not reproduce issue #14…
message_e88a21   dev-issue-15  dispatcher  report  2026-09-13T11:58:44+09:00  waiting  Addressed the 3 comments and CI…
```

## runtime

Reads installed Runtime Adapters and their Runtime metrics.

```text
feld runtime <COMMAND>
```

### feld runtime list

Lists installed [Runtimes](../spec/concepts/glossary.md#runtime) with the number of running Sessions.

```text
feld runtime list
```

```console
$ feld runtime list
NAME         LABEL        RUNNING_SESSIONS
claude-code  Claude Code  3
codex        Codex        1
```

### feld runtime metrics

Shows Runtime metrics: the 5-hour / 7-day quota per account, and the Session metrics of all Workspaces aggregated per Runtime and account. Feld has metric rules for `claude-code` and `codex`; for other Runtimes, quota and metrics are `n/a`.

```text
feld runtime metrics [OPTIONS] [RUNTIME]
```

**Arguments**

- `[RUNTIME]`: Runtime name. If omitted, all installed Runtimes

**Options**

- `--since <DURATION>`: Length of the range going back from now. A positive integer and a unit (`m` / `h` / `d`), such as `30m`, `5h`, or `7d`. Default `5h` (the 5-hour quota window). Cannot be combined with `--start` / `--end`
- `--start <TIME>`: Start of the range. Default 5 hours before the end of the range
- `--end <TIME>`: End of the range. Default now
- `--bucket <SECONDS>`: Bin width of the `--json` series. 60 or more. Requires `--json`

The first line of the output shows the range read (`Range: last 5h (<start> – <end>)`, or `Range: <start> – <end>` with `--start` / `--end`). `--json` holds the range in `start` / `end`. Quota lines add the account's display name, and reports with an unknown account show only the window name. Metric lines show the account's display name in the `ACCOUNT` column, and values of Sessions with an unknown account are `unknown`.

```console
$ feld runtime metrics claude-code
Range: last 5h (2026-09-13T09:10:00+09:00 – 2026-09-13T14:10:00+09:00)

Runtime claude-code
Quota
  5-hour me@example.com:   62% used · resets 2026-09-13T14:00:00+09:00
  5-hour bot@example.com:  18% used · resets 2026-09-13T16:00:00+09:00
  7-day me@example.com:    31% used · resets 2026-09-17T09:00:00+09:00
  7-day bot@example.com:   9% used · resets 2026-09-18T11:00:00+09:00
METRIC                   ACCOUNT          STATUS    VALUE
token usage              me@example.com   reported  3511820 total
token usage              bot@example.com  reported  390294 total
estimated cost           me@example.com   reported  $34.10
estimated cost           bot@example.com  reported  $4.30
API errors               me@example.com   reported  3
API errors               bot@example.com  reported  0
prompt cache hit rate    me@example.com   reported  90%
prompt cache hit rate    bot@example.com  reported  84%
API request latency p95  me@example.com   reported  9110 ms
API request latency p95  bot@example.com  reported  7240 ms
tool calls               me@example.com   reported  1102
tool calls               bot@example.com  reported  106
context window           me@example.com   reported  71% (max of sessions)
context window           bot@example.com  reported  22% (max of sessions)
active time              me@example.com   reported  262m 14s
active time              bot@example.com  reported  40m 3s
```

## provider

Reads installed Execution Providers.

```text
feld provider <COMMAND>
```

### feld provider list

Lists installed [Execution Providers](../spec/concepts/glossary.md#execution-provider) with their Nodes and the number of running Sessions.

```text
feld provider list
```

```console
$ feld provider list
NAME   LABEL  NODES  RUNNING_SESSIONS
tmux   tmux   local  3
herdr  herdr  local  1
```

## self

Uses the [Runtime API](../spec/concepts/glossary.md#runtime-api) as the Session this command runs in. Call it from inside a Session. The Session is determined by a token from the first three entries of the resolution order in [Common conventions](#common-conventions) (`FELD_SESSION_TOKEN`, or the self index of the calling Execution Provider or Runtime Adapter). If none is found, it prints nothing to standard output, writes the reason (`No Session token: run this inside an attached or owned Feld Session`) to standard error, and exits with exit code 1.

```text
feld self <COMMAND>
```

### feld self get

Shows the calling Session. A closed Session is rejected with `session closed`, exit code 1.

```text
feld self get
```

```console
$ feld self get
Session session_9c41e2
  actor:            dev-issue-15 (agent)
  workspace:        default
  generation:       1
  start:            2026-09-13T10:42:07+09:00
  end:              -
  start_kind:       start (owned) · spawned_by dispatcher (session_0aa4d9)
  end_kind:         running
  outcome:          running
  execution:        agent-runtime · claude-code
  model:            claude-fable-5-1
  placement:        tmux · local · window @7 (main:1, created) · ~/Dev/feld-issue-15
  start_settled:    2026-09-13T10:42:09+09:00
  observation:      running (2026-09-13T11:50:00+09:00) · the recorded tmux incarnation is running
  attach_command:   tmux -L feld attach-session -t '@7'
```

### feld self token

Prints only the value of the calling Session's token to standard output. A program inside a Session uses it to call the daemon API directly with the token. It resolves locally without connecting to the daemon, so inside a sandbox that denies reading the process table it answers only with the token from the environment, and if there is none it exits with `cannot verify the self index from inside a sandbox that denies reading the process table` and exit code 1. How the token is passed in `owned` and `attached` is in [Session token distribution](../spec/design/auth.md#distributing-the-session-token).

```text
feld self token
```

```console
$ feld self token
<Session token>
```

### feld self message recv

Takes unread Messages from the calling Actor's mailbox in FIFO order, shows them, and records a `read` (`sessionId`, `at`, `readId`, `fifo`) for each Message. The model uses it to read on its own, and it produces the same records as the Runtime Adapter's helper. The next call sees only what arrived after that. If nothing is unread, it shows `No pending messages.`

```text
feld self message recv [OPTIONS]
```

**Options**

- `--wait`: If nothing is unread, wait until a Message arrives

If interrupted before the read is recorded, the same Message is handed over again on the next take. If writing the body fails because the stdout reader closed, it records no read, exits with exit code 2, and shows the same Message again on the next call. If writing fails because the reader closed after the read was recorded, it keeps the read, exits with exit code 2, and writes to stderr the IDs of the Messages whose read was recorded and how to re-read them with `feld message get <message-id>`.

```console
$ feld self message recv
Messages (1)
--- message_f2a9d0 from Admin at 2026-09-13T11:45:03+09:00
Please address the review comments
Mailbox read committed.
  read:         read_7d31c4 (message_f2a9d0, fifo 12)
  session:      session_9c41e2
  at:           2026-09-13T11:45:20+09:00
```

## ui

### feld ui

Signs in and opens the Workspace's Feld Web UI in a browser. The daemon issues a pairing code named `local`, writes owner-only to `$FELD_HOME/admin/ui-pair/` a page that moves to the Sign in screen URL carrying the code in its fragment, and opens that page. Only the page's path appears in the browser's argv, and the next `feld ui` removes expired pages. The Web UI exchanges the code for a browser token cookie and key and moves to the Workspace's Actors page. The browser token that browser holds is replaced with the new one. The browser is `BROWSER`, or else whatever the platform opens links with.

```text
feld ui [OPTIONS]
```

**Options**

- `--print-url`: Show the URL without opening a browser. The URL works once within 5 minutes

```console
$ feld ui --print-url
http://127.0.0.1:7381/ui/pair?workspace=default#code=K7QM-4XPD
```

### feld ui pair

Issues a pairing code for a browser on another device to Sign in, and shows the code, name, and expiry. Enter the code on the Sign in screen of the Web UI opened through an HTTPS proxy (`tailscale serve`). It works once within 5 minutes. The daemon does not know the address visible from outside the proxy, so no URL is shown. The procedure is in [pairing in auth.md](../spec/design/auth.md#pairing).

```text
feld ui pair [OPTIONS]
```

**Options**

- `--name <NAME>`: Name of the browser token issued on exchange. Default `browser`

```console
$ feld ui pair --name laptop
Sign-in code issued.
  code:       K7QM-4XPD
  name:       laptop
  expires_at: 2026-10-01T10:47:09+09:00
Sign in to the Web UI of the other device with the code within 5 minutes. It works once.
```

## browser

Manages the browser tokens of signed-in browsers. These commands call with the local token, so use them on the daemon's machine.

### feld browser list

Lists valid browser tokens.

```text
feld browser list
```

```console
$ feld browser list
ID                                         NAME    SIGNED_IN                  LAST_USED                  EXPIRES
browser_5c1f0e9a2b7d4c86a3e1f4b2d9c07a51  laptop  2026-10-01T10:42:31+09:00  2026-10-01T11:05:12+09:00  2026-10-08T11:05:12+09:00
browser_9e2d41c7b0a84f3fb6c5d2e1a7f80c34  local   2026-09-28T09:10:02+09:00  2026-10-01T09:58:40+09:00  2026-10-08T09:58:40+09:00
```

### feld browser revoke

Revokes a browser token and signs that browser out. That browser's next request is rejected with 401, and the Web UI returns to the Sign in screen.

```text
feld browser revoke <NAME|ID>
```

**Arguments**

- `<NAME|ID>`: The browser token's `id`, or a name. A name revokes all valid tokens with that name

```console
$ feld browser revoke laptop
Browser signed out.
  id:         browser_5c1f0e9a2b7d4c86a3e1f4b2d9c07a51
  name:       laptop
  revoked_at: 2026-10-01T11:20:03+09:00
```

## feld env

Outputs, as `export` statements, the environment that binds a shell to the Feld home's daemon. The values are `FELD_HOME`, the daemon API (`FELD_API`), and variables such as `OTEL_*` that send Runtime telemetry to the daemon's Collector. `OTEL_*` is the same as the Claude Code [launch environment](../spec/design/adapter/claude-code.md#launch-arguments-and-environment) of an `owned` Session plus `CLAUDE_CODE_OTEL_HEADERS_HELPER_DEBOUNCE_MS`. The API and Collector endpoints are values kept in the Feld home and stay the same across daemon restarts, so they can be output even while the daemon is stopped. For a home where the daemon has never started, it outputs only `FELD_HOME`. [`feld init`](#feld-init) appends `eval "$(feld env)"` to the shell config file, and people [attach](#feld-actor-attach) Runtimes started from that shell.

```text
feld env
```

```console
$ feld env
export FELD_HOME=/Users/alice/.feld
export FELD_API=http://127.0.0.1:7381
export CLAUDE_CODE_ENABLE_TELEMETRY=1
export OTEL_METRICS_EXPORTER=otlp
export OTEL_LOGS_EXPORTER=otlp
export OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
export OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318
export OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=delta
export OTEL_METRIC_EXPORT_INTERVAL=10000
export CLAUDE_CODE_OTEL_HEADERS_HELPER_DEBOUNCE_MS=60000
```

## launchSpec options

Accepted by `actor create` and `actor update`. What runs is either `--runtime` (execution `agent-runtime`) or `-- <COMMAND>...` (execution `program`).

- `--runtime <RUNTIME>`: The [Runtime Adapter](../spec/concepts/glossary.md#runtime-adapter) the Session runs. `claude-code` or `codex`. Cannot be given together with `-- <COMMAND>`
- `--runtime-settings <JSON>`: Settings the Runtime Adapter reads. A single JSON object, passed to the Runtime Adapter as is. Cannot be given together with `-- <COMMAND>`. Rejected if it contains keys the Adapter builds itself (`ownedSettings` in the manifest)
- `--trust-workdir`: Make the Runtime load the workdir's settings and hooks without asking (sets the launchSpec's `trustWorkdir` to true). Only an Admin token can set it. Cannot be given together with `-- <COMMAND>`
- `--no-trust-workdir`: Remove `trustWorkdir` in `actor update`
- `--workdir <PATH>`: The Session's working directory. A relative path resolves from the current directory. Default the current directory. Validated before declaring; if it is not an existing, enterable directory on the `local` node, it is rejected without declaring anything
- `--env <KEY=VALUE>`: An environment variable added to the Session's launch environment. Repeat to give several. In `actor update`, the given set of variables replaces the whole set. Variables Feld sets (such as `FELD_*`) and the Runtime's settings (Claude Code's `runtimeSettings.env`, Codex's `runtimeSettings.shell_environment_policy.set`) take precedence over values with the same name. Output such as `actor get` shows only variable names; `--json` shows the values
- `--provider <PROVIDER>`: The [Execution Provider](../spec/concepts/glossary.md#execution-provider) that hosts the Session. `tmux` or `herdr`. Default the one the Provider options point to, else the Provider the CLI runs in, else the only installed Provider
- `--node <NODE>`: The Node that runs the Session. Default `local`
- `--<provider>-<key> <VALUE>`: Provider-specific choices. The CLI passes the key without the prefix and the value to the Provider as is, as `providerOptions`. The Provider defines the choices and their defaults, and omitted choices are filled from the caller's environment at declaration time ([tmux](../spec/design/provider/tmux.md#resolving-the-three-axes), [herdr](../spec/design/provider/herdr.md#resolving-the-three-axes)). Feld decides the Session's Placement from these at launch

**tmux**

- `--tmux-socket <NAME|PATH>`: The tmux socket. A path if it contains `/`. Default the socket of the tmux the CLI runs in, or `feld` outside tmux
- `--tmux-unit <session|window|pane>`: The unit that hosts the Session. Default `session`
- `--tmux-session <NAME>`: The existing tmux session in which `--tmux-unit window` creates a window
- `--tmux-window <ID>`: The existing tmux window in which `--tmux-unit pane` creates a pane

**herdr**

- `--herdr-session <NAME|PATH>`: The herdr server. If it contains `/`, the path of its socket. Default the `HERDR_SOCKET_PATH` of the herdr the CLI runs in, or `default` outside herdr
- `--herdr-unit <workspace|tab|pane>`: The unit that hosts the Session. Default `workspace`
- `--herdr-workspace <ID>`: The existing workspace in which `--herdr-unit tab` creates a tab
- `--herdr-pane <ID>`: The existing pane that `--herdr-unit pane` splits
- `--herdr-split <right|down>`: The split direction of `--herdr-unit pane`. Default `right`

## Internal commands

Hidden from help; Feld itself launches them.

- `feld daemon run`: The daemon process that `feld daemon start` launches
- `feld collector-run`: The OTel Collector the daemon launches
- `feld program-capture --output <PATH> --stdout-fifo <PATH> --stderr-fifo <PATH> --done <PATH>`: Launched by the capture wrapper that wraps a program's command. Writes stdout / stderr line by line to output records and creates the `--done` file on completion
