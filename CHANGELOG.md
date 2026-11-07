# Changelog

## 0.3.0

### Highlights

- `feld diff` / `feld apply` declare the standing Actors of a Workspace in a YAML file and make the Workspace match it, with secrets kept out of the store through `envFile`.
- Actors carry labels with an optional `until` and `reason`, and a Python SDK (`feld-sdk`) ships with each Release.
- Behind an authenticating proxy, `feld daemon start --anonymous-read` serves the Web UI read-only to a team without pairing.

### Breaking

- **daemon**: `PATCH /v1/workspaces/{ws}/actors/{actor}` (`feld actor update` / `pause` / `resume`) answers `409 session closed` instead of `401` to the token of a closed Session (#31)
  - Affects: scripts that update Actors with a Session token and treat `401` as a closed Session
  - Fix: treat `409` with `session closed` the same way

### Added

- **cli, daemon**: `feld diff -f <file>` / `feld apply -f <file>` apply the Actors declared in a YAML file through the new `PUT /v1/workspaces/{ws}/actors/{actor}` (`?dryRun=true` for diff). Fields the file leaves out are kept, and `apply --prune` deletes only the Actors the same file declared before (#37)
- **daemon, cli**: launchSpec takes `envFile`, a file of `KEY=VALUE` lines the daemon reads at each launch, so secret values never reach the store. `feld actor create/update --env-file` / `--no-env-file` (#37)
- **daemon, cli, webui**: an Actor carries labels (`key`, optional `until` and `reason`), set with `feld self label` / `feld actor label` and the `labels` API. Lapsed labels are removed within about a second, and every change is recorded as `label.set` / `label.cleared` / `label.expired`. The Web UI shows them as chips on the Actor band (#39)
- **sdk**: the Python SDK `feld-sdk` (Python 3.11+) with `feld.connect()`, a synchronous client grouped by resource, and `feld.testing.FakeDaemon`. Each Release attaches its wheel and sdist (#36)
- **daemon, cli, webui**: `feld daemon start --anonymous-read` answers token-less `GET` / `HEAD` as Admin and refuses writes with 403, and the Web UI draws itself read-only with a Read-only badge. `GET /v1/ui/access` tells the caller's access (#47)
- **daemon, cli**: a Session whose Actor holds `control` can pause and resume the Schedule of any Job in its Workspace, recorded as `schedule.paused` / `schedule.resumed` with the requester (#31)
- **cli**: `feld doctor` checks for a newer Release on GitHub and warns with the upgrade step (#30)
- **webui**: with All workspaces selected, the Actors section draws one foldable lane per Workspace, with counts and New actor in each heading (#34)

### Fixed

- **webui**: the Metrics chart stays beside the pinned lane labels at mid widths (#43)
- **webui**: Console tabs, links and toggles keep their own font, color and focus outline (#42)
- **webui**: the Console event summary meets WCAG AA contrast (#41)
- **test**: tests that start tmux servers pass in deep checkouts (#44)

## 0.2.1

### Highlights

- An idle daemon on Linux no longer spins a core and grows its memory until the OOM killer takes it. Upgrade if you run Feld on Linux.
- `daemon.log` now records each start and the daemon's CPU and memory when they climb, so a later crash leaves a trail.

### Added

- **daemon**: `daemon.log` starts each run with the version, start time and pid, and logs the CPU share, peak RSS, live tokio tasks and uptime when the daemon turns busy (half a core or more) or calm, and each time its peak RSS passes 256 MiB or doubles (#27)

### Changed

- **deps**: minimatch 11.0.4 under `@redocly/openapi-core` for GHSA-g3hv-75wg-m249 (#28)

### Fixed

- **daemon**: on Linux, the spool watch ignores the open events that its own scans produce, so an idle daemon no longer wakes itself in a loop and fills memory (#27)

## 0.2.0

### Highlights

- The compact layout of the Web UI (phones and narrow windows) is rebuilt around a one-row top bar and a navigation drawer. The bottom tab bar is gone, each Actor band shows its name row over a full-width Timeline, and the Console opens full screen.
- The Actions menu and the range and Metrics pickers can now be reached from the keyboard.
- Dependencies are updated for RUSTSEC-2026-0381, RUSTSEC-2026-0382, RUSTSEC-2026-0386 and the vite, vitest and happy-dom advisories.

### Added

- **web**: a navigation drawer and a one-row top bar replace the bottom tab bar in the compact layout; Actor bands lay out as a name row and a full-width Timeline; the Settings tabs, Console lines and range picker fit the compact width (#12)
- **web**: the compact Console opens full screen and closes from the identity bar (#7)
- **web**: the Actions menu and the range and Metrics pickers are reachable from the keyboard; the focus stays inside the open drawer and returns to the menu button after a page change (#12, #13)
- **docs**: Japanese and Simplified Chinese Quickstart (#4), the Quickstart rewritten around an attached Claude Code (#6), and installing Feld with Homebrew (#5)

### Changed

- **deps**: webpki-roots 1.0.10 (RUSTSEC-2026-0381), rustls-webpki 0.103.16 (RUSTSEC-2026-0382), vite 8.3.4 (launch-editor advisories), tokio 1.54.1 (RUSTSEC-2026-0386), vitest 5.0.5 (GHSA-qxg6-89mg-rqv8, GHSA-xp26-3454-mjgp) and happy-dom 21.0.4 (GHSA-834v-8h66-3h79) (#7, #26)
- **cli, docs**: a read-only Codex thread is resumed with `--sandbox workspace-write`; the hint no longer suggests `--yolo` (#18)
- **core**: the cron computation moves from model to `feld_core::cron` (#11)
- **docs**: the How it works diagram is rendered as images (#3)
- **tests**: the Vitest, spool signal-cancel and poll_mailbox tests hold under `make check`, and the Provider switch of `actor update` is checked end to end (#10, #14, #20, #21)

### Fixed

- **web**: the compact top bar spans the full width on every page, so the Settings page no longer shows the page background beside it (#7)
- **web**: Metrics lanes size to their stacked labels in the compact layout, so a label no longer overlaps the next lane (#8)
- **web**: Escape keeps working in the Message composer while a Message is being sent (#19)
- **daemon**: every browser cookie of the same name is tried when signing in (#16)
- **codex**: the initial prompt is passed after `--`, so a prompt that starts with `-` is not read as a flag (#17)
- **herdr**: a stop input that herdr would read as its own global option is refused (#22)
- **install**: `install.sh` defaults to the `crestai-labs/feld` repository (#2)
- **repo**: `node_modules/` at the repository root is ignored (#9)

## 0.1.0

Initial release.
