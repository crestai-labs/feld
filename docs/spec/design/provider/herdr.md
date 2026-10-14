# herdr Execution Provider

This document describes the internal behavior of the [Execution Provider](../../concepts/glossary.md#execution-provider) `herdr` when it creates, identifies, observes, and stops the entity of a Session on herdr. The contract with core is in [plugin.md](../plugin.md), and the comparison with the tmux Provider is in [tmux.md](tmux.md).

## Contents

- [Role](#role)
- [Deciding the Placement](#deciding-the-placement)
  - [Resolving the three axes](#resolving-the-three-axes)
  - [created and existing](#created-and-existing)
  - [incarnation](#incarnation)
- [Launch](#launch)
- [IDs and current names](#ids-and-current-names)
- [observation](#observation)
  - [Decision procedure](#decision-procedure)
  - [herdr server stop and restart](#herdr-server-stop-and-restart)
- [attach](#attach)
  - [`--herdr-pane`](#--herdr-pane)
  - [`--self`](#--self)
  - [Candidates](#candidates)
  - [self index](#self-index)
- [Stopping](#stopping)
  - [stop](#stop)
  - [kill](#kill)
  - [Cleanup](#cleanup)

## Role

The herdr Provider covers the same boundary as the tmux Provider (process management running under the same OS account as Feld) on top of the herdr server. In herdr, a background server owns the panes (real terminals) and arranges them in a workspace > tab > pane hierarchy. The Provider asks this server to create, identify, and stop resources through the `herdr` CLI. CLI commands that correspond to API operations answer with one line of JSON (`{"id":"cli:…","result":{…}}`, or `{"id":…,"error":{"code":…}}` on failure), and `pane run` and `close` print nothing on success. `status server --json` answers the server state as JSON that includes `socket` and other fields. Below, operations are written with their raw API names, and the CLI form is given alongside where it differs.

The Provider handles only execution resources; the [Runtime Adapter](../../concepts/glossary.md#runtime-adapter) decides what to launch and how to communicate with it. Everything the Provider reports is an observation from the outside: the kind of an [observation](../../concepts/glossary.md#observation) is decided only by matching the pane and processes, and [turn](../../concepts/glossary.md#turn) is derived from the transcript. The agent status herdr derives per pane, prompt / wait between agents, and native session restore after a server restart are herdr features, and an agent inside a Session can use them directly through the herdr skill. On top of that, Feld holds the Session's record, token, Message, and metrics.

The only `node` the Provider handles is `local`, and the daemon rejects any node other than `local` in Actor creation and update and in attach. The incarnation fields read from the process table identify an entity on the same host as the daemon.

## Deciding the Placement

### Resolving the three axes

The [Placement](../../concepts/glossary.md#placement) of an `owned` Session has the same three axes as tmux, mapped onto the herdr hierarchy. The launchSpec holds the selectors a person writes (session name, workspace / pane ID), and the Placement holds the IDs herdr resolves them to.

- `endpoint`: the value of `--herdr-session`. A session name is recorded as the server of `herdr --session <name>`, and a path as the socket of `HERDR_SOCKET_PATH`. When omitted, it is the `HERDR_SOCKET_PATH` that `self` read in the CLI that requested the launch, or the default session for a call from outside herdr. The herdr API reaches only a running server, so a person keeps the endpoint's server running with `herdr`
- `unit`: the value of `--herdr-unit`. Defaults to `workspace`
- `borrowed`: when `unit` is `tab`, the existing workspace named by `--herdr-workspace`; when `pane`, the existing pane named by `--herdr-pane` (the split source). Empty for `workspace`
- `--herdr-split`: the split direction when `unit` is `pane` (`right` / `down`). Defaults to `right`

Feld resolves borrowed to an ID and a terminal id when it accepts the launch request, and confirms again, just before creating resources, that the same entity answers. If the server or the parent has been replaced after resolution, it creates no resources and treats this as a [launch](#launch) failure.

### created and existing

- `unit` is `workspace`: workspace `wN`, its first tab `wN:t1`, and root pane `wN:p1` are `created`
- `unit` is `tab`: tab `wN:tM` and root pane `wN:pK` are `created`, and workspace `wN` is `existing`
- `unit` is `pane`: the split pane `wN:pK` is `created`, and the tab and workspace are `existing`
- attach: the pane, tab, and workspace are all `existing`. `unit` is `pane`, and `borrowed` is that pane itself

The Provider has side effects only on `created` resources, and among them it acts on the narrowest one (in the order: the pane's program, pane, tab, workspace).

### incarnation

herdr pane IDs keep increasing within one workspace, and after a server restart they are restored with the same numbers along with the saved layout. A restored pane is a new shell, and the original processes do not remain. So the Provider distinguishes panes by pairing the pane ID with the terminal id herdr assigns to the pane's real terminal (`term_…`; newly assigned on each restore and not saved). herdr does not answer the server's pid through the API, so a server replacement appears as a change of terminal id.

The Provider identifies one resource by the following tuple.

- endpoint
- workspace `wN`, tab `wN:tM`
- pane `wN:pK` and terminal id. A pane moved to another workspace gets a new ID, and its terminal id stays the same
- the pid and start time of the pane's process (`shell_pid` of `pane.process_info`). Launch and attach both fail on a pane that does not answer `shell_pid`, so a Placement always has it
- the pid and start time of the program's process (a child of the pane's process)
- for an `attached` Session, the pid and start time of the Runtime process (identified in [attach](#attach))

The Placement records this tuple after creating the resources (for attach, at attach time), and every later observation and stop first matches against this tuple.

## Launch

1. Resolve endpoint, unit, and borrowed from the launchSpec, and save them as the Placement in one transaction with the Session record. It does not have an incarnation yet
2. Reconfirm that the borrowed workspace / pane is as recorded (for a pane, the terminal id also matches)
3. Create resources according to unit. Pass `workdir` as `cwd` and, in `env`, `FELD_LAUNCH_SCRIPT` holding the script injected in step 4, and keep focus as it is. Receive the IDs and terminal id of the created resources from the response. `HERDR_ENV`, `HERDR_PANE_ID`, `HERDR_SOCKET_PATH`, and the other variables herdr puts in the pane's process remain as they are, and they are what lets the `feld` CLI inside the Session answer `self`

   ```console
   $ herdr workspace create --cwd <workdir> --label <name> --env FELD_LAUNCH_SCRIPT=… --no-focus
   $ herdr tab create --workspace w3 --cwd <workdir> --label <name> --env FELD_LAUNCH_SCRIPT=… --no-focus
   $ herdr pane split w3:p1 --direction right --cwd <workdir> --env FELD_LAUNCH_SCRIPT=… --no-focus
   ```

   The raw API operations are `workspace.create`, `tab.create`, and `pane.split`, and the split-source pane ID the CLI takes as a positional argument corresponds to `target_pane_id` of `pane.split`

4. The created pane runs a shell. Confirm with `pane.process_info` that the shell is in the foreground (the foreground process group is `shell_pid` itself), and inject the following line with `pane.send_input` (`text` and `keys: ["enter"]`; `pane run` in the CLI). Through `exec`, the pane's process keeps the same pid as it passes from the shell to `sh`, and after `<command>` ends, to `sleep`

   ```sh
   exec sh -c "$FELD_LAUNCH_SCRIPT"
   ```

   Input to the pane goes through the terminal, so a line can hold a few hundred bytes. The script itself is passed as `FELD_LAUNCH_SCRIPT` in step 3, and this line holds only its name. The script removes itself from the environment, receives from the Provider the environment variables the Runtime Adapter assembled, and then launches `<command>` ([Passing the launch environment](../plugin.md#passing-the-launch-environment)). The Provider passes the environment to the `shell_pid` process

   ```sh
   unset FELD_LAUNCH_SCRIPT; eval "$(<plugin> launch-env <socket> <Provider pid> || echo exit 1)"; <command>; s=$?; printf '\033]2;feld exit %d\007' "$s"; exec sleep 2147483647
   ```

   `<command>` is the argv the Runtime Adapter assembled, and it runs as a child process of `sh`. When it ends, `sh` writes the exit code into the pane's terminal title (OSC 2) and waits with the pane kept open. This corresponds to tmux's `remain-on-exit` and dead pane, and the Provider can read the exit code from `terminal_title` of `pane.get`

5. Build the incarnation from the terminal id in the response of step 3, `shell_pid` of `pane.process_info`, and the program's pid and start time read from the process table, observe, and record the incarnation and the first observation in one transaction. The program is the single child that newly appears under the pane's process after it becomes the `sh` of the line in step 4 and remains. Children the shell that read the line creates for its prompt, and the `launch-env` that receives the environment, fall outside this condition. If the observation is `running`, its time becomes `startSettled`. If `<command>` has already ended, the observation is `gone`, and the Session closes without `startSettled`, with the exit code from the terminal title

When the reconfirmation in step 2 or the command in step 3 fails and it is certain that no resources were created, record `gone`. When it is uncertain whether resources were created, record `lost`. When the command in step 4 fails, close the created resources and record `gone`, or record `lost` when it is uncertain that they were closed. Handling when the daemon stops before recording the incarnation is in [session-lifecycle.md](../session-lifecycle.md#recovery-after-a-daemon-restart).

The wait in step 5 includes the time until the shell a person uses finishes starting and reads the line. If, within the wait, the pane's process does not become the launch script's `sh` and the terminal title has no exit code, the line is treated as not having reached the shell, the same as a step 4 failure. A Placement without a program can match no observation and no stop reaches it, so the launch fails here.

## IDs and current names

The Placement saves herdr IDs (`w3` / `w3:t1` / `w3:p1`) and the terminal id. The workspace `label` and the tab number (`feldtest:2`) are for display; the Provider has herdr resolve them on each operation that reads herdr (`observe`, `launch`, `attach`) and adds them to `summary`. A current name is attached only while the pane is still inside the resource of the recorded unit; otherwise (after `pane.move`) no current name is attached.

The attach command is derived from the endpoint and the pane's terminal id. herdr's direct terminal attach opens one pane in that terminal.

```console
$ herdr --session <name> terminal attach term_abc123
$ HERDR_SOCKET_PATH=<path> herdr terminal attach term_abc123
```

## observation

### Decision procedure

The Provider matches the recorded incarnation from the outside and returns one observation kind. It decides in the following order and takes the first one that is settled. Below, "program" means the `<command>` process for an `owned` Session and the Runtime process for an `attached` Session.

1. For an `attached` Session, check the Runtime process's pid and start time in the process table. If the pid is absent or the start time differs, `gone`. The Runtime process is not a child of the Provider, so no exit code is available, and it is reported as null
2. Query the endpoint's server for panes, tabs, and workspaces together with `session.snapshot` (`api snapshot` in the CLI), and look for the pane with the recorded terminal id. The Placements one `observe` receives are decided with this one read per endpoint. If the server cannot be reached (`server_not_running`, or `server_unavailable` while stopping), go to [server stop and restart](#herdr-server-stop-and-restart)
3. No pane with that terminal id: if the recorded pane ID has another terminal, `mismatched` (another entity is under the same ID). If the ID is also absent (`pane_not_found` from `pane.get`), check the program in the process table: `lost` if the recorded incarnation is still running, `gone` if the pid is absent or the start time differs. The pane is closed, so the exit code is null
4. The pane exists, but the pid and start time of the recorded pane's process (`shell_pid` of `pane.process_info`) do not match in the process table: `mismatched`
5. The pane and the pane's process match, and the program is running: `running` if the pane ID, tab, and workspace in the response are also as recorded. `relocated` if `pane.move` changed the pane ID, tab, or workspace. The Placement stays as is, and the current position is attached to the observation. herdr keeps the pre-move pane ID as an alias, so a stop reaches it with the recorded ID
6. The pane and the pane's process match, and the program has ended: `gone`. Report `feld exit <n>` in `terminal_title` as the exit code, or null if absent. The pane is read before the process table, so when the program has ended and the title read has no exit code yet, read the pane again
7. If reading herdr or the process table fails along the way, `unknown`

A Session that has not recorded its incarnation yet cannot be matched. While the [launch](#launch) procedure is in progress, no observation is recorded, and if a daemon restart interrupted the procedure, `lost` is returned. Observation triggers are the same as in [tmux.md](tmux.md#observation-triggers).

### herdr server stop and restart

When the server cannot be reached, the pane state cannot be read, but the program can be checked in the process table. Return `lost` if the recorded program is still running, and `gone` (exit code null) if the pid is absent or the start time differs. herdr ends the pane's processes when the server stops, so this is mostly `gone`.

When the server runs again, panes are restored with the same IDs, but the terminal id and processes are new, so step 3 of the [decision procedure](#decision-procedure) gives `mismatched`. When herdr's native session restore resumes the Runtime in a restored pane, that is a different incarnation for Feld, and running `feld actor attach --self` from inside it creates a new `attached` Session. What happens next follows the admission rules ([session-lifecycle.md](../session-lifecycle.md)).

## attach

In attach, the Provider pins an existing pane and the Runtime process running in it to an incarnation and hands them to the Control Plane. The Provider's only operation on the attached pane and Runtime process is observation.

The Runtime process is identified by combining the following two.

- the leader of the foreground process group in `pane.process_info`. A shell makes a program launched in the foreground the parent of its own process group, so the pid of `claude` launched from the shell is this leader
- matching of the executable name and argv by the Runtime Adapter's `match`

If matching fails, attach is rejected. Handling of attach to the same Runtime incarnation is the same as in [tmux.md](tmux.md#attach).

### `--herdr-pane`

The CLI passes `w1:p2`. The Provider reads `pane.get` and `pane.process_info` at the endpoint of `--herdr-session` (when omitted, by the same rule as launch), takes the workspace, tab, terminal id, and the pid and start time of `shell_pid`, and records them as an `existing` pane placement. `workdir` is `foreground_cwd`, or `cwd` if absent. The selector's endpoint is the server's `socket` that the CLI's `status server --json` answers.

### `--self`

`self` reads the CLI's environment variables `HERDR_ENV` (`1`), `HERDR_SOCKET_PATH`, and `HERDR_PANE_ID`, and answers the endpoint (in the path form of `HERDR_SOCKET_PATH`) and the pane as the selector, and `attach` records it by the same procedure as `--herdr-pane`. The Runtime process is the process among the CLI's ancestors whose pid matches the pane's foreground process group, and it is recorded if it passes the Runtime Adapter's matching. `--self` is accepted for calls from inside a herdr pane. The pane also has `HERDR_SESSION`, but the endpoint is normalized to the path form so that one pane has one self index key.

### Candidates

`candidates` reads all panes of the server at the selector's socket (or the same default as launch if absent) with `session.snapshot`, and for each pane answers the leader of the foreground process group in `pane.process_info` as the foreground process. Panes that do not answer `shell_pid` or a foreground process, panes where either of those processes is not running, and panes that answer neither `foreground_cwd` nor `cwd` cannot be attached and are excluded. `options` are `pane` (`wN:pK`) and `session` (the server's socket path), `workdir` is `foreground_cwd` (or `cwd` if absent), and `summary` has the form `pane w1:p2 (main:1)`. The CLI checks whether the foreground process is a Runtime with the Runtime Adapter's `match`.

### self index

core builds the self index key from the provider name `herdr` and the selector `attach` answers (socket path and pane ID). When `--herdr-session` is a name, `attach` resolves it to the server's socket path and answers that. `--herdr-pane` and `--self` write the same index. The read and write procedure is as in [auth.md](../auth.md#attached-sessions), and a process running inside the pane looks up the index with the same key from the answer of `self`.

## Stopping

stop and kill both record `attempt` and `observed` in `lifecycle.stop` / `lifecycle.kill`. The Provider matches the incarnation just before trying, and causes side effects only when it is `running` or `relocated`. Otherwise, it returns that observation as `observed` as is.

### stop

Deliver an exit request suited to the execution to the recorded pane.

- `agent-runtime` with `stopInput`: send `stopInput` to the pane with `pane.send_input` (`text` and `keys: ["enter"]`; `pane run` in the CLI). An empty input is just Enter, which corresponds to `pane send-keys <pane> enter` in the CLI. Send inputs one at a time, re-observe after each, and send the next only while the entity remains. If the entity still remains after the whole sequence is sent, resend the sequence according to the [`stopInput`](../plugin.md#manifest) rules. `attempt` is `herdr sent /exit to the pane`
- `program`, and `agent-runtime` without `stopInput`: send SIGTERM to the program's process. `attempt` is `herdr sent SIGTERM to the pane's program`

Then re-observe within a finite wait; once it is `gone`, record it in `observed` and close the Session. If the program still remains after the wait, record `observed` as `the pane and its program are still present`, and the Session accepts `feld session kill` without an `end`.

### kill

Send SIGKILL to the program's process. `attempt` is `herdr killed the pane's program (SIGKILL)`. SIGKILL is sent with `kill -KILL <pid>`, and the pid's start time is matched just before and just after sending; if the pid passed to another process in between, `observed` becomes `unknown`. Once re-observation gives `gone`, record it with the exit code (usually 137) and close the Session.

### Cleanup

When a Session closes as `gone`, the Provider cleans up only `created` resources.

- `unit` is `workspace`: `workspace.close`
- `unit` is `tab`: `tab.close`
- `unit` is `pane`: `pane.close` when other panes remain in the borrowed tab. If the created pane is the last pane in that tab, leave the pane as is to keep the borrowed tab

When herdr closes a pane, it sends SIGHUP, SIGTERM, and SIGKILL to its process in that order. When the server is not running, there is nothing to close, so the resources are left with that reason. When the server runs again, the resources are restored with the saved layout, and a person closes them. The borrowed workspace / tab and the `existing` pane attached by attach are left as they are. The incarnation is matched before cleanup as well, and only entities that match the record are targeted. When the recorded pane has left the `created` resources (after `pane.move`), the resources are left as is, as an arrangement a person has rearranged, with that reason attached.

Workspaces Feld creates are outside worktree groups. A workspace that herdr asks to close together with its group is left as is, with that reason.
