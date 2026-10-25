# Changelog

## 0.2.0

### Highlights

- The compact layout of the Web UI (phones and narrow windows) is rebuilt around a one-row top bar and a navigation drawer. The bottom tab bar is gone, each Actor band shows its name row over a full-width Timeline, and the Console opens full screen.
- The Actions menu and the range and Metrics pickers can now be reached from the keyboard.
- Dependencies are updated for RUSTSEC-2026-0381, RUSTSEC-2026-0382 and the vite launch-editor advisories.

### Added

- **web**: a navigation drawer and a one-row top bar replace the bottom tab bar in the compact layout; Actor bands lay out as a name row and a full-width Timeline; the Settings tabs, Console lines and range picker fit the compact width (#12)
- **web**: the compact Console opens full screen and closes from the identity bar (#7)
- **web**: the Actions menu and the range and Metrics pickers are reachable from the keyboard; the focus stays inside the open drawer and returns to the menu button after a page change (#12, #13)
- **docs**: Japanese and Simplified Chinese Quickstart (#4), the Quickstart rewritten around an attached Claude Code (#6), and installing Feld with Homebrew (#5)

### Changed

- **deps**: webpki-roots 1.0.10 (RUSTSEC-2026-0381), rustls-webpki 0.103.16 (RUSTSEC-2026-0382) and vite 8.3.4 (launch-editor advisories) (#7)
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
