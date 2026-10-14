# tmux Execution Provider

This document describes the internal behavior of the [Execution Provider](../../concepts/glossary.md#execution-provider) `tmux` when it creates, identifies, observes, and stops the entity of a Session on tmux. The contract with core is in [plugin.md](../plugin.md).

## Contents

- [Role](#role)
  - [Scope](#scope)
- [Deciding the Placement](#deciding-the-placement)
  - [Resolving the three axes](#resolving-the-three-axes)
  - [created and existing](#created-and-existing)
  - [incarnation](#incarnation)
- [Launch](#launch)
- [Stable IDs and current names](#stable-ids-and-current-names)
- [observation](#observation)
  - [Decision procedure](#decision-procedure)
  - [tmux server restart](#tmux-server-restart)
  - [Observation triggers](#observation-triggers)
- [attach](#attach)
  - [`--tmux-pane`](#--tmux-pane)
  - [`--self`](#--self)
  - [Candidates](#candidates)
  - [self index](#self-index)
- [Stopping](#stopping)
  - [stop](#stop)
  - [kill](#kill)
  - [Cleanup](#cleanup)

## Role

The tmux Provider launches the entity of a Session as a local process, makes it independent of the life of the launching terminal and of the daemon, and makes it rediscoverable after a daemon restart. It is a process-management boundary that runs under the same OS account as Feld. Other Providers (`docker`, `docker-sandbox`) provide the isolation boundary, and the [herdr Provider](herdr.md) provides the same boundary on herdr.

The Provider handles only execution resources. The [Runtime Adapter](../../concepts/glossary.md#runtime-adapter) decides what to launch and how to communicate (launch arguments, environment, hook, helper), and the Provider feeds that command and environment into a pane. Everything the Provider reports is an observation from the outside and is distinct from what the Runtime or the Session itself declares. The Provider returns the tmux-specific items of the Placement (the three axes, stable IDs, incarnation) as `placement`, and core stores them as they are.

### Scope

The tmux versions the tmux Provider runs on are in [the supported versions in plugin.md](../plugin.md#supported-versions).

The Provider always calls tmux with `-u` and reads the fields of format output separated by the private-use character U+E000. Every version from the minimum onward returns U+E000 unchanged, regardless of the caller's locale.

## Deciding the Placement

### Resolving the three axes

Feld decides the [Placement](../../concepts/glossary.md#placement) of an `owned` Session at launch from the provider-specific options in the launchSpec. The launchSpec holds the selectors a person writes (socket name, session name, a window name like `main:1`), and the Placement holds the stable IDs that tmux resolves them to.

- `endpoint`: the value of `--tmux-socket`. A name is recorded as `-L <name>`, and a path as `-S <path>`. When omitted, it is the socket path indicated by the `TMUX` environment variable that `self` read in the CLI that requested the launch, or Feld's default socket name `feld` for a call from outside tmux
- `unit`: the value of `--tmux-unit`. Defaults to `session`
- `borrowed`: when `unit` is `window`, the existing session pointed to by `--tmux-session`; when `pane`, the existing window pointed to by `--tmux-window`. Empty for `session`

When Feld accepts a launch request, it resolves endpoint and borrowed to stable IDs and the server's incarnation. For borrowed, Feld checks existence with tmux via `has-session -t` and then searches with `list-panes -a`. If it is not found, Feld answers with `window <window> was not found on tmux socket <socket>` / `session <session> was not found on tmux socket <socket>`, and if there is no server, with `the borrowed tmux resource has no running server`. A server that is shutting down after its last session closed answers with zero sessions or with a disconnection such as `server exited unexpectedly`, so Feld resolves it as having no server. Right before creating resources, Feld checks again that the same incarnation answers. If the server or the parent has been replaced after resolution, Feld creates no resources even when the names are the same, and treats it as a failure of [Launch](#launch). A replacement Session rereads the launchSpec and gets a new Placement.

### created and existing

For each resource, the Placement holds whether Feld created it (`created`) or it already existed (`existing`). This is the origin of the resource and is a separate item from the Session's [ownership](../../concepts/glossary.md#ownership).

- `unit` is `session`: session `$n`, its first window `@n`, and pane `%n` are `created`
- `unit` is `window`: window `@n` and pane `%n` are `created`, session `$n` is `existing`
- `unit` is `pane`: pane `%n` is `created`, window `@n` and session `$n` are `existing`
- attach: pane `%n`, window `@n`, and session `$n` are all `existing`. `unit` is `pane`, and `borrowed` is that window

The Provider applies side effects only to `created` resources, and among them operates on the narrowest one (in the order: the pane's process, pane, window, session).

### incarnation

An incarnation is a tuple that tells whether the entity present now is the same one that was recorded. IDs like tmux's `%3` and OS pids are reassigned to another entity after the entity disappears. tmux IDs increase only within the lifetime of a single server, and the next server numbers again from `$0` / `%0`. So the Provider pairs the number with the time the entity was born to tell them apart. For example, suppose it recorded `claude` with pid 4812, started at 10:00. If this `claude` ends and another process gets pid 4812 at 11:00, its start time is 11:00, so it is known to be different from the recorded entity.

The Provider identifies a resource by the following tuple.

- endpoint
- the tmux server's pid and the process start time
- session `$n` and `#{session_created}`
- window `@n`
- pane `%n`, and the pid and start time of the pane's process (`#{pane_pid}`). In an `owned` Session, `<command>` takes over the pane's process with `exec`, so this is also recorded in `program` as the program's process
- in an `attached` Session, the pid and start time of the Runtime process (identified in [attach](#attach))

The Placement records this tuple after creating resources (at attachment, for attach), and every later observation and stop is performed after matching against this tuple. If even one item differs, the entity is considered a different one.

## Launch

1. Resolve endpoint, unit, and borrowed from the launchSpec, and store them as the Placement in one transaction with the Session record. It does not yet hold an incarnation
2. Recheck that the borrowed session / window and the server's incarnation match the record
3. Depending on unit, create the resource with a command that only waits. Create it with `-d`, pass `workdir` to `-c`, and use `-P -F` to receive at once the stable IDs of the created resources, the server's pid, `#{session_created}`, and the `#{pane_pid}` of the waiting command

   ```console
   $ tmux -L feld new-session -d -s <name> -c <workdir> -P -F '<creation format>' -- 'exec sleep 2147483647'
   $ tmux -L feld new-window -d -t '$3:' -n <name> -c <workdir> -P -F '<creation format>' -- 'exec sleep 2147483647'
   $ tmux -L feld split-window -d -t '@7' -c <workdir> -P -F '<creation format>' -- 'exec sleep 2147483647'
   ```

4. Set `remain-on-exit` on the created pane. From then on, even after the process ends, the pane remains as a dead pane and the Provider can read the exit code (`#{pane_dead_status}`)

   ```console
   $ tmux -L feld set-option -p -t '%3' remain-on-exit on
   ```

5. `<command>` is a single shell statement that launches argv with the environment variables, both assembled by the Runtime Adapter. `<command>` receives the environment variables from the Provider and imports them ([passing the launch environment](../plugin.md#passing-the-launch-environment)). The environment variables enter only the pane's process, and the environment of the tmux server and the session stays as it is. It then waits on the gate `feld-launch-<server pid>-<n>` (`<n>` is the number of `%n`) with `wait-for`, and when the gate opens, launches argv with `exec`. The program takes over the shell's pid, and `#{pane_pid}` points to the program itself

   ```console
   eval "$(<plugin> launch-env <socket> <Provider pid> || echo exit 1)"; tmux -L feld wait-for feld-launch-<server pid>-<n>; exec <argv>
   ```

6. Replace the waiting command with `<command>` via `respawn-pane -k`, and reread the pane with `display-message`. Check that the session, window, server pid, and `#{session_created}` read match those created in 3, and that the server and the `#{pane_pid}` process are in the process table; pass the environment to the `#{pane_pid}` process, and then open the gate with `wait-for -S`. `remain-on-exit` is in effect before `<command>` starts running, so the exit code remains even for a Job that ends immediately. When many panes are launched at once, tmux can fail to fork and `respawn-pane` can fail. tmux 3.2a–3.7c leaves no input state on the failed pane, and the next `respawn-pane` or similar on that pane ends the whole server, so the Provider sends only kill to that pane, closes the created resources, and retries from 3 once

   ```console
   $ tmux -L feld respawn-pane -k -t '%3' -c <workdir> -- '<command>'
   $ tmux -L feld display-message -p -t '%3' '<pane format>'
   $ tmux -L feld wait-for -S feld-launch-<server pid>-3
   ```

7. Build the incarnation from the received stable IDs and pids and the start times read from the process table, observe, and record the incarnation and the first observation in one transaction. If the observation is `running`, its time becomes `startSettled`. If `<command>` has already ended, the observation is `gone` of a dead pane, and the Session closes without `startSettled`, with `#{pane_dead_status}` as the exit code

When the recheck in 2 or the command in 3 fails and it can be confirmed that no resources were created, `gone` is recorded. When tmux cannot create the server's socket, it ends with exit code 0; 3.4 and later print the reason only to stderr, and 3.2 prints nothing. Therefore, when 3 ends with exit code 0 and prints no output, it is also treated as a failure if there is stderr or the `-S` socket does not exist. When it cannot be confirmed whether resources were created, `lost` is recorded. When 3 succeeds but its output cannot be read, the `%n` pane contained in the output is reread with `list-panes -a`. Only when the session `$n`, window `@n`, `%n`, server pid, `#{session_created}`, and `#{pane_pid}` of that pane, joined with one separator, match the output, and also match the incarnation if the server was checked in 2, are the created resources closed, as on a failure of 4 or 6. If this cannot be confirmed, the resources are left and `lost` is recorded. When a command in 4 or 6 fails, or the reread in 6 does not match, the created resources are closed, and `lost` is recorded when it cannot be confirmed that they were closed. When they are closed, `gone` is recorded, except on the first `respawn-pane` failure, which retries from 3. The handling when the daemon stops before the incarnation is recorded is in [session-lifecycle.md](../session-lifecycle.md#recovery-after-a-daemon-restart).

When the Provider starts a tmux server in 3, that server holds the environment of the Provider process (`FELD_HOME`, `FELD_API`, `FELD_PLUGIN_DIR` in [plugin.md](../plugin.md)) in its global environment, visible to every pane of that server. The Provider removes `FELD_SESSION_TOKEN`, `FELD_SESSION_ID`, `FELD_ACTOR`, and `FELD_WORKSPACE`, which point to one Session, from the environment it calls tmux with. These reach only the pane's process, as the environment variables of 5.

## Stable IDs and current names

The Placement stores only stable IDs (`$1` / `@7` / `%3`). Current names like `main:1` are for display; the Provider has tmux resolve them on every read and adds them to `summary`. tmux answers the current name, and it is attached only while the pane is still within that resource; otherwise only the stable ID is shown.

The attach command is derived from `endpoint` and the widest of the `created` resources. `-t` is given `$n` when `unit` is `session`, `@n` when `window`, and `%n` when `pane`. If the pane has moved out of that resource, the pane's `%n` is given, which opens the window and pane where the pane is now.

```console
$ tmux -L feld attach-session -t '@7'
```

## observation

### Decision procedure

The Provider matches the recorded incarnation from the outside and returns one [observation](../../concepts/glossary.md#observation) kind. The decision goes in the following order, and the first one settled is taken.

1. In an `attached` Session, check the pid and start time of the Runtime process in the process table. If the pid is absent or the start time differs, `gone`. The Runtime process is not a child of tmux, so no exit code is available, and it is reported as null. If the Runtime process is running, go to the next step
2. Check the pid and start time of the endpoint's tmux server in the process table. If they differ from the record (the pid is absent, or the start time differs), go to [tmux server restart](#tmux-server-restart)
3. Read all panes on the endpoint with `list-panes -a` and look for the recorded `%n`. If there are no panes at all, or the `#{pid}` of any pane differs from the recorded server pid, the endpoint is connected to a different server, so `lost` (`the endpoint is connected to a different tmux server`)
4. `%n` is absent: check the pane's process (the Runtime process in an `attached` Session) in the process table. If the recorded incarnation is still running, `lost` (tmux let go of the pane but the process is alive); if the pid is absent or the start time differs, `gone`
5. `%n` exists and is a dead pane (`#{pane_dead}`): `gone`. tmux 3.8 and later answer `#{pane_pid}` of a dead pane as empty, so this decision does not look at `#{pane_pid}`. `#{pane_dead_status}` is reported as the exit code. For a process ended by a signal, the exit code is derived from the signal (`term` is 143, `kill` is 137, number `n` is 128 + n). If `#{pane_dead_status}` / `#{pane_dead_signal}` are both empty, tmux has not yet reaped the process, so the Provider rereads with `display-message` within a finite wait (2 seconds) before reporting. On every reread, regardless of version, it does the following two things. tmux before 3.6 built with utempter drops a `SIGCHLD` that arrives while it is closing the dead pane's terminal and does not reap that process, so the Provider sends `SIGCHLD` to the tmux server to make it reap. tmux 3.2, which lacks `#{pane_dead_signal}`, leaves both empty even for a process ended by a signal, so the Provider reads the last line tmux draws in the pane, `Pane is dead (signal term, <datetime>)`, with `capture-pane -J`, which joins the parts wrapped in a narrow pane, and gets the signal
6. `%n` exists, but `#{pane_pid}` or the process start time differs from the record: `mismatched`. A different entity is under the same name. When `#{pane_pid}` matches the record and that pid is absent from the process table, it is treated as a match, as a process in the middle of exiting, and the decision continues with the next steps until the pane becomes dead
7. `%n` and the process match, and the session `$n` / window `@n` containing it also match the record: `running`
8. `%n` and the process match, but the containing session / window has changed (`break-pane`, `join-pane`, `move-window`, link to another session): `relocated`. The Placement is kept as it is, and the current location (the stable IDs and current names of the window and session containing the pane) is added to the observation's `detail`. Stopping targets `%n` and the pid, so it reaches the entity even while relocated
9. If reading tmux or the process table fails along the way, `unknown`. A read failure is uncertainty, and is distinct from both evidence of the entity's presence or absence and permission for side effects

A Session that has not yet recorded an incarnation cannot be matched. While the [Launch](#launch) procedure is in progress, no observation is recorded, and if the procedure was interrupted by a daemon restart, `lost` is returned.

The record is written only when the kind changes from the previous one, and the latest observation time is updated as `observation.at`.

### tmux server restart

When the server's incarnation differs from the record, every recorded stable ID loses its basis for re-identification. The Provider returns `lost`, and identification uses only stable IDs and the incarnation. Feld leaves `lost` in the observation history and surfaces the Session in [attention](../../concepts/glossary.md#attention) while keeping it current.

What follows obeys the admission rules ([session-lifecycle.md](../session-lifecycle.md)). The next launch request or stop / kill closes the previous Session with `unknown`, and for a launch request, the launchSpec selectors are resolved again on the new server to get a new Placement. A stop / kill on a `lost` Session only records `attempt` as `nothing to kill` and `observed` as `lost`, and closes it.

### Observation triggers

- Right after launch: record the first `running` and settle `startSettled`
- Periodic observation: the daemon periodically observes Sessions without `end` together in a single `observe`. An unrequested exit (`exited`) is recorded here by observing `gone`
- `POST /v1/sessions/{sessionId}/observe` (`feld session observe`, `feld actor list --refresh`): observe and record now, and return the result
- After stop / kill: observe right after the attempt, and record it as `observed`
- daemon startup: re-observe the entity of each Session restored from the store

Admission of a launch request reads the latest stored observation. When it is rejected because of `unknown` or no observation, observe again with `--refresh` and then request again.

## attach

What the Provider does in attach is to fix the existing pane and the Runtime process running in it to an incarnation and hand them to the Control Plane. The Provider's only operation on the attached pane and Runtime process is observation. An `attached` Session tracks the Runtime process, and when the Runtime process ends, closes as `gone` even if the pane and shell remain ([Decision procedure](#decision-procedure)).

The Runtime process is identified by combining the following two.

- The parent of the foreground process group on the pane's terminal (`#{pane_tty}`). The shell makes a program launched in the foreground the parent of an independent process group, so the pid of `claude` launched from the shell is this one
- Matching of the executable name and argv by the Runtime Adapter's `match`. This confirms that the candidate is the Runtime of `--runtime`

If matching fails, attach is rejected.

If the pid and start time of the identified Runtime process match the pid and start time of the Runtime process (in an `owned` Session, the program's process) recorded in the current Session of any Actor, it is an attach to the same Runtime incarnation, and the daemon creates no new Session. The branching after that is in [attach in session-lifecycle.md](../session-lifecycle.md#attach).

### `--tmux-pane`

The CLI passes values like `%3` or `main:1` as they are. The Provider resolves the value on the endpoint of `--tmux-socket` (when omitted, by the same rule as launch). A value that is not `%n` is checked for existence with tmux via `has-session -t` and then turned into `%n` with `display-message -p '#{pane_id}'`. It then reads `list-panes -a`, takes the session `$n` and window `@n` containing that pane, the server's pid and start time, the pid and start time of `#{pane_pid}`, and `#{pane_current_path}`, and records them as an `existing` pane placement. If that pane is not in the list, it answers with `pane <pane> was not found on tmux socket <socket>`. `workdir` becomes `#{pane_current_path}`. `--tmux-pane` points to a pane where the Runtime is in the foreground. The Provider matches the parent of that pane's foreground process group with the Runtime Adapter, and if it passes, records its pid and start time as the Runtime process.

### `--self`

`self` reads the CLI's environment variables `TMUX` (`<socket path>,<server pid>,<session number>`; the Provider reads only the socket path) and `TMUX_PANE` (`%n`), and answers the endpoint (path form) and pane (`%n`) as the selector; `attach` records it by the same procedure as `--tmux-pane`. The Runtime process is the process in the CLI's parent chain (parent, its parent, ...) whose pid matches the pane's foreground process group, and it is recorded if it passes the Runtime Adapter's matching. The CLI runs from a Runtime tool as a descendant of the Runtime, so the Runtime is in the parent chain. In a call where the Runtime Adapter's `self` answers no selector, `--self` is accepted for a call from inside a tmux pane; any other call is rejected as lacking a Placement.

When the Runtime runs its Runtime-side session outside the pane's foreground process (Codex's managed app-server), the tool is not a descendant of the foreground process, and `TMUX_PANE` can point to another pane. In this case, if the Runtime Adapter's `self` answers a selector, a person chooses the pane ([plugin.md](../plugin.md#division-of-work-on-attach)). `--self` is accepted with `--tmux-pane` (and `--tmux-socket`) added, and records the added pane by the same procedure as `--tmux-pane` without checking the parent chain. `--self` without them is rejected, showing the [Candidates](#candidates) panes.

### Candidates

`candidates` answers all live panes of the server on the selector's socket (if none, the same default as launch). `options` is `pane` (`%n`) and `socket` (socket path), `process` is the parent of the foreground process group, `workdir` is `#{pane_current_path}`, and `summary` takes the form `pane %433 (main:2.1)`. The CLI checks whether the foreground process is the Runtime with the Runtime Adapter's `match`.

### self index

Core builds the self index key from the provider name `tmux` and the selector `attach` answers (the endpoint's socket path and `%n`). Both `--tmux-pane` and `--self` write the same index. `--self` to a pane a person chose writes to the Runtime's key. The read and write procedure is as in [auth.md](../auth.md#attached-sessions), and a process running in the pane looks up the index with the same key from the answer of `self`.

## Stopping

Both stop and kill record "what the Provider attempted (`attempt`)" and "what it observed right after the attempt (`observed`)" in `lifecycle.stop` / `lifecycle.kill`. The Provider matches the incarnation right before attempting, and causes side effects only when it is `running` or `relocated`. When it is `gone` / `lost` / `mismatched` / `unknown`, the Provider returns that observation as `observed` as it is. In the re-observation after an attempt, `mismatched` is treated as a transient state in the middle of exiting, and observation continues during the wait.

### stop

The Provider delivers a termination request suited to the execution to the recorded `%n` pane.

- `agent-runtime` with `stopInput`: send the `stopInput` of the Runtime Adapter's manifest (for Claude Code, `/exit` and the answer to the confirmation) to the pane with `send-keys -t '%n'`. The inputs are sent one at a time as lines, re-observing after each, and the next one is sent only while the entity remains. If the entity remains after the whole sequence has been sent, the sequence is sent again according to the rules of [`stopInput`](../plugin.md#manifest). `attempt` is `tmux sent /exit to the pane`
- `program`, and `agent-runtime` without `stopInput`: send SIGTERM to the `#{pane_pid}` process. `attempt` is `tmux sent SIGTERM to the pane's process`

After that, the Provider re-observes within a finite wait, and when it becomes `gone`, records it in `observed` and closes the Session. If the pane and process still remain after the wait, `observed` is recorded as `the pane and its process are still present`, and the Session, still without `end`, accepts `feld session kill`.

### kill

Send SIGKILL to the `#{pane_pid}` process. `attempt` is `tmux killed the pane's process (SIGKILL)`. Signals are sent with the `kill -<signal> <pid>` command (`kill -KILL <pid>` for SIGKILL), and the same holds for the SIGTERM of stop and the `SIGCHLD` of the [Decision procedure](#decision-procedure). For SIGKILL, the pid's start time is matched right before and right after sending, and if the pid has passed to another process in between, `observed` becomes `unknown`. When re-observation shows `gone`, it is recorded with the exit code (usually 137), and the Session closes.

### Cleanup

When a Session closes as `gone`, the Provider cleans up only `created` resources.

- `unit` is `session`: `kill-session -t '$n'`
- `unit` is `window`: `kill-window -t '@n'`
- `unit` is `pane`: `kill-pane -t '%n'`. The number of panes is counted in the window the pane is in now. If the pane is in a borrowed window and is its last pane, the dead pane is left as it is to keep the borrowed window. A pane moved to another window by `break-pane` or `join-pane` is closed where it moved to as well, and tmux closes that window together with the pane only when the pane was the last one

The borrowed session / window and an `existing` pane attached by attach are left as they are. The incarnation is matched before cleanup as well, and only entities matching the record are targeted. A dead pane that answers `#{pane_pid}` as empty (tmux 3.8 and later) is matched only by `%n`, which the server does not reuse.
