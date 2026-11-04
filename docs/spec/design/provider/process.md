# process Execution Provider

This document describes the internal behavior of the [Execution Provider](../../concepts/glossary.md#execution-provider) `process` when it launches, observes, and stops the entity of a Session as an OS process. The contract with core is in [plugin.md](../plugin.md).

## Contents

- [Role](#role)
- [Placement](#placement)
- [Launch](#launch)
- [observation](#observation)
- [Stopping](#stopping)
- [Cleanup](#cleanup)

## Role

The process Provider runs a Job's `program` as an OS process of its own, with the workdir and the environment of each launch. No terminal multiplexer stands between Feld and the program, so a Job shares no server's working directory, environment, or life with other Sessions, and runs where no tmux or herdr is installed. Like the [tmux Provider](tmux.md), it runs under the same OS account as Feld, and the entity it launches is independent of the daemon's life and rediscovered after a daemon restart.

The program has no terminal, so a person cannot attach to it, and an Agent does not run on it ([Selection from launchSpec](../plugin.md#selection-from-launchspec)). The Console reads the program's stdout / stderr through core's capture wrapper, the same as on any Provider. `providerOptions` takes no keys; `resolve` answers `{}`. `self` answers null, and `attach` and `candidates` refuse.

## Placement

| Field | Meaning |
|---|---|
| `workdir` | The directory the program starts in |
| `state` | The state directory of this launch, `$FELD_HOME/providers/process/<name>-<id>` (owner-only), where the supervisor records the exit |
| `supervisor` | The pid and start time of the supervisor |
| `program` | The pid and start time of the program; null when the program ended before the launch read it |

`summary` is `process <pid>`, and `attachCommand` is null.

## Launch

1. `place` checks that `workdir` is an existing absolute directory, and refuses the start otherwise
2. `launch` creates the state directory and starts the Plugin's own executable as `<plugin> supervise <state>` in a new session (`setsid`), with `/` as its working directory and nothing on stdout but a pipe back to the launch
3. The launch reads the supervisor's pid and start time while the supervisor waits on its stdin, then writes the command, the environment, and the workdir to that stdin as JSON and closes it. The environment travels only on this pipe ([token exposure prevention](../auth.md#preventing-token-exposure))
4. The supervisor starts the program with the workdir as its working directory, `PWD` set to it, the environment it received on top of its own (without `FELD_SESSION_TOKEN`, `FELD_SESSION_ID`, `FELD_ACTOR`, `FELD_WORKSPACE` of the supervisor's own environment), and `/dev/null` as stdin, stdout, and stderr. It answers the program's pid as one line on its stdout, or the reason the program could not start, and then lets go of stdout
5. The launch reads the program's start time, observes, and returns the Placement with the first observation

When the program cannot start (the workdir is gone, the command cannot be run), or the supervisor does not answer within the deadline, the launch kills the supervisor's process group, removes the state directory, and answers `gone` without a Placement.

The supervisor waits for the program and writes `{"exitCode": <n>}` to `<state>/exit`, through a temporary file renamed over it. A program ended by a signal has the exit code 128 + the signal number.

## observation

The decision goes in the following order, and the first one settled is taken.

1. `<state>/exit` exists: `gone` with its exit code
2. The pid and start time of `program` are in the process table: `running`
3. The supervisor is in the process table: the program has ended and the exit is being recorded. The Provider rereads `<state>/exit` within a finite wait (2 seconds) and answers `gone` with its exit code; if the supervisor is still there without a record, `unknown`
4. Neither is in the process table: `gone` with a null exit code
5. If reading the process table or the state directory fails, `unknown`

## Stopping

stop sends SIGTERM and kill sends SIGKILL to the `program` process, after matching its incarnation. A `stopInput` is delivered as SIGTERM, since the program reads no terminal. `attempt` is `process sent SIGTERM to the program` / `process killed the program (SIGKILL)`. After the attempt the Provider re-observes within a finite wait (2 seconds), and a program still running is recorded as `the program is still running`, leaving the Session open for `feld session kill`.

## Cleanup

When a Session closes as `gone`, the Provider removes the state directory. While the program or the supervisor still runs, it keeps the directory.
