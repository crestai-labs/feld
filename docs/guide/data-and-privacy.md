# Data & Privacy

Feld does not send the user's data outside. What Feld reads, what it writes, and how to remove it are shown below.

The Runtime config root refers to `CLAUDE_CONFIG_DIR` (or `~/.claude` if unset) for Claude Code, and `CODEX_HOME` (or `~/.codex` if unset) for Codex.

## What Feld does not send

- The daemon listens on loopback (`127.0.0.1`, and `[::1]` in environments with IPv6) and does not communicate with external servers. The CLI and the OpenTelemetry Collector reach the daemon on the same machine, and the metrics sent by Plugins (such as Claude Code status line values) reach the Collector on the same machine.
- Runtime hooks and mailbox helpers (the Claude Code Plugin Monitor and the Codex queue helper) send requests with the Session token to the daemon on the same machine through the socket in the Feld home, send the Runtime identity, and receive Messages.
- Feld does not send usage statistics (data the developers collect about usage). Feld's Collector receives Runtime (Claude Code, Codex) telemetry inside the user's machine to display metrics, and is separate from usage statistics.
- If usage statistics are introduced, they will be opt-in (sent only when the user enables them), and announced as prominently as `### Breaking` in the [Release notes](https://github.com/crestai-labs/feld/releases).
- The Web UI can be opened from another device when the user configures `tailscale serve` and `feld daemon start --allow-host` ([CLI reference](cli.md#feld-daemon-start)).
- Communication outside the machine happens through `install.sh` at installation (fetches the GitHub Release) and the Runtime's own communication (according to each Runtime's settings).

## What Feld reads

| Target | Location | Items read |
|---|---|---|
| Claude Code transcript | `<config root>/projects/<project>/<session id>.jsonl`, and for Sub Agents `<session id>/subagents/agent-<id>.jsonl` and `.meta.json` | Conversation (prompts, responses, thinking, tool calls and results) and model. Read on the spot when the Web UI Console displays it |
| Codex rollout | `<config root>/sessions/<YYYY>/<MM>/<DD>/rollout-<start>-<thread id>.jsonl` | Conversation (prompts, responses, reasoning, tool calls and results), context window and token counts, rate limit |
| Claude Code account | `CLAUDE_CONFIG_DIR/.claude.json` (or `~/.claude.json` if unset) | Reads the whole file and uses `oauthAccount.accountUuid` and `oauthAccount.emailAddress` |
| Codex account | `<config root>/auth.json` | Reads the whole file and uses `tokens.account_id` (or the account id in `tokens.id_token` if absent) and the email in `id_token`. Other items (access token, refresh token, API key, etc.) are neither stored nor sent |
| OpenTelemetry | Metrics and logs the Runtime sends to the daemon's Collector | Stored in the store as received. Up to 64 MiB per Session, and data for closed Sessions is deleted after 30 days. Includes attributes the Runtime attaches, such as `user.email`. Settings that include prompts (such as `OTEL_LOG_USER_PROMPTS`) are the user's choice |
| Hook input | JSON the Runtime passes to hooks | `session_id`, `transcript_path`, `model`, `hook_event_name` |
| Claude Code status line input | JSON Claude Code passes to the status line | Context window and rate limit. The whole input is passed as is to the user's own status line command |
| Runtime settings | Claude Code `<config root>/settings.json`, Codex `<config root>/hooks.json` and `<config root>/config.toml` | Items that `feld init` and `feld uninstall` rewrite ([What Feld writes](#what-feld-writes)) |
| Processes | `ps`, and the environment variables of the Claude Code process being attached | Start time, command, parent process. For environment variables, checks whether `OTEL_EXPORTER_OTLP_ENDPOINT` points to Feld's Collector |

## What Feld writes

### Feld home

`~/.feld` (`FELD_HOME`) is a directory only the owner can open (`0700`). `admin/` holds files only Admin reads, and the rest are also read by processes inside Sessions.

| Location | Contents |
|---|---|
| `admin/feld.db` (and `-wal`, `-shm`) | store. Workspaces, Actors and launchSpecs, Sessions, Messages, Telemetry |
| `admin/api-token`, `admin/collector.json`, `admin/ui-pair/` | local token, Collector settings, the `feld ui` Sign in page |
| `sessions/<id>/` | Per-Session files. Job output (`console/output.jsonl`) and files the Runtime Adapter passes to the Session (`runtime/settings.json` for owned Claude Code) |
| `daemon.log` (and `.1`–`.4`) | daemon log. Rotated every 10 MB, keeping 5 generations |
| `plugins/` | Plugins placed by `feld init` |
| `prepared/` | Original values of Runtime settings replaced by `feld init` |
| `tokens/`, `self-index/`, `daemon.json`, `daemon.sock`, `spool/`, etc. | Session tokens, daemon and Collector records, the socket through which Runtime hooks and helpers reach the daemon (placed in `feld-<uid>/` in the temporary directory when the Feld home path is too long for a socket), spool, locks |

### Runtime config roots

`feld init` writes these through the Runtime Adapter. For items that replace or chain onto the user's existing values, and items that widen the Runtime's sandbox, it shows the values to write and asks for consent (`--yes` gives consent).

- Claude Code: `skills/feld/` (a Claude Code plugin holding the skill, hooks, Plugin Monitor, and helpers), and `otelHeadersHelper` and `statusLine` in `settings.json`
- Codex: `skills/feld/` (skill and helpers), Feld's hooks in `hooks.json`, and the spool (`<Feld home>/spool/shared`) in `writable_roots` of `[sandbox_workspace_write]` in `config.toml`. Codex records hook trust in `config.toml`

### Shell config files

`eval "$(feld env)"` is a single line the user places in a shell config file (`~/.zshrc` for zsh, `~/.bashrc` for bash, etc.). Runtimes started from that shell send telemetry to Feld's Collector.

### Secrets in records

- Conversation records (Claude Code transcripts and Codex rollouts) and Job output are stored unmodified and displayed as is in the Web UI Console. Feld masks only Feld's own tokens.
- Keep secrets out of launchSpec arguments (`command`) and `runtimeSettings` (including environment variables). The launchSpec is stored in the store and readable through the API and the Web UI.
- When you configure access from another device (`tailscale serve`) or share your screen, secrets in records appear on screen.

## Removing data

| Means | Removed | Kept |
|---|---|---|
| `feld uninstall` | What was written to the Runtime config root (restored to the original values; values the user changed after `feld init` are kept) and the Feld home settings (Plugins, tokens, daemon records and logs, spool) | `sessions/` and the store |
| `feld uninstall --purge` | In addition to the above, `sessions/` and the store | — |

- The daemon deletes Telemetry by its limit and retention period ([What Feld reads](#what-feld-reads)). The way to actually delete other records is `feld uninstall --purge` (deletes the whole history). `feld actor delete`, `feld workspace delete`, and `feld message delete` only hide items from lists, and the records remain in the store.
- Claude Code transcripts and Codex rollouts are managed by the Runtime and remain in each Runtime's config root.
- Remove the line in the shell config file by hand. The `next:` output of `feld uninstall` shows the file containing that line.
