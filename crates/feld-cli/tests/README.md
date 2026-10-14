# feld tests

Tests that drive the `feld` CLI and daemon from the outside. They run with `cargo test -p feld` and are part of `make test` and `make check`.

## What runs for real and what is replaced

| Target | How the tests treat it |
|---|---|
| daemon, CLI, Plugins | The real builds. `make build` places the Plugin executables next to `feld` |
| tmux, herdr | The real ones. Where they are missing, the tests print the reason to stderr and pass. Building herdr from source requires Zig 0.16.0 |
| Claude Code, Codex | A stand-in written in C (`support::compile_runtime`). It starts, runs one command if needed, and waits. The tests attach to it, read it, and end it. The hook and spool parts come from the real Plugins; the transcripts and metrics use real output recorded in `fixtures/`. Where `cc` is missing, tests that use the stand-in print the reason and pass |
| Real Claude Code, Codex | Only `real_claude_code.rs` and `real_codex.rs` run them, and they call the APIs. They run only when `FELD_TEST_REQUIRE` lists them |

## What the automated tests do not prove

- That the real Claude Code and Codex emit hooks, transcripts, and metrics of the same shape as the stand-in. `real_*.rs` and manually triggered CI runs (on the minimum and latest versions) check this
- Web UI rendering. The vitest tests in `web/` import modules directly and check them
- How a terminal a person attached to feels to use

## What the tests leave alone

The tests write nothing to the environment of the person running them, and read only from the following three places.

| What is protected | How |
|---|---|
| `~/.feld` | Each test gives its own home with `--home` or `FELD_HOME` |
| `~/.claude`, `~/.codex` | `support::feld()` drops `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, and so on, and the fixture points `HOME` to its own location. Tests that run a real Runtime pass their own root |
| The tmux server of the person running the tests | Fixtures use their own socket with `-S`. Named servers with `-L` (including the Provider's default `feld`) are placed under `target/` by `TMUX_TMPDIR` in `.cargo/config.toml` |
| Outside Feld Sessions, tmux and herdr panes | `support::feld()` drops `FELD_*`, `HERDR_*`, `TMUX`, and `TMUX_PANE`. Tests that run the daemon inside the test process (such as `scheduler.rs`) inherit the process environment, so `make test` drops the same variables |
| herdr settings | `support::HerdrServer` starts a headless server with its own root directly under `/tmp` and its own session, and removes it at the end |

Read only: one test each in `claude_code_session.rs`, `codex_session.rs`, and `herdr_provider.rs` takes the modification times of the real `~/.claude/settings.json` and `~/.codex`, and checks after the test that they have not changed.

The tmux servers under `target/` remain after the tests. `tmux -L <name> kill-server` or `cargo clean` removes them.

## Running the real Claude Code and Codex

List product names (`tmux`, `herdr`, `claude-code`, `codex`) separated by commas in the environment variable `FELD_TEST_REQUIRE`, and the matching tests fail where that product is missing. When `tmux` is listed, they also fail where `cc` is missing. Listing an unknown name makes the tests fail.

`real_claude_code.rs` runs the real Claude Code both in a Session that Feld starts and in a Session started from a shell prepared by `feld init` and brought in with `/feld attach`, and calls the Anthropic API. It runs only when `FELD_TEST_REQUIRE` lists `claude-code`, and then it requires `claude` on `PATH` and `ANTHROPIC_API_KEY`.

```sh
FELD_TEST_REQUIRE=tmux,claude-code ANTHROPIC_API_KEY=... cargo test -p feld --test real_claude_code
```

`real_codex.rs` runs the real Codex in the `workspace-write` sandbox, both in a Session that Feld starts and in a Session started with `codex --no-daemon` from a shell prepared by `feld init` and brought in from outside with `feld actor attach --tmux-pane`, and calls the OpenAI API. The startup flag `--dangerously-bypass-hook-trust` takes care of the hook trust prompt. It runs only when `FELD_TEST_REQUIRE` lists `codex`, and then it requires `codex` on `PATH` and `OPENAI_API_KEY`.

```sh
FELD_TEST_REQUIRE=tmux,codex OPENAI_API_KEY=... cargo test -p feld --test real_codex
```
