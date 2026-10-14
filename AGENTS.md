# Agent Development Guide

## The Feld project

Feld is an open, harness-agnostic **Agent Loop Control Plane**.

## Repository layout

- `crates/`: the Rust workspace (details on the crates and Plugins are in [`CONTRIBUTING.md`](CONTRIBUTING.md#repository-layout)). `feld` (the CLI; it starts the daemon), `feld-daemon` (the API, serving the Web UI, Session control, the scheduler, the store, metrics), `feld-core` (what the daemon, the CLI, and the Plugins share), `feld-plugin-<name>` (the Runtime Adapter and Execution Provider executables)
- `plugins/`: Plugin manifests, and the hooks, monitors, and skills installed into Runtimes
- `web/`: the Web UI. Its tech stack and code conventions are in [`web/README.md`](web/README.md)
- `docs/spec/`: the source of truth for the spec the implementation satisfies. It consists of concepts (`concepts/`) and design (`design/`). The reading order and precedence are in [`docs/spec/README.md`](docs/spec/README.md)
- `docs/guide/`: documents for the surfaces through which people and programs touch Feld (Quickstart, CLI, daemon API, Web UI, compatibility, Data & Privacy). They are written and updated with the current implementation as the truth. The daemon API's OpenAPI definition, `docs/guide/daemon-api.openapi.yaml`, is a contract that the `feld-daemon` tests check against the routes

Keep the precedence `docs/spec` > implementation > `docs/guide`. When changing `Feld`, check consistency with the higher-ranked artifacts and update them as needed.

## Checking changes

- Pass `make check` before opening a PR. Build and test steps, and how to write commits and PRs, are in [`CONTRIBUTING.md`](CONTRIBUTING.md)
- When you change the Web UI source, rebuild `web/dist/` with `make web` and include it in the same commit

## Documentation rules

- Leave out the history (for example, do not write "the idea of ~ was rejected" or "~ is not adopted"). Write only how things should be
- Write in the affirmative where possible and avoid the negative (for example, do not write "~ is not adopted"). Write only how things should be
- Avoid duplication and do not write too much. Leave out what is already written elsewhere and what is self-evident. Keep the text concise.

## Rust code conventions

### Where tests go

- As a rule, write unit tests in `#[cfg(test)] mod tests { ... }` in the same file as the code under test
- When a test module exceeds 300 lines or grows longer than the code itself, move it to a separate file
  - Whether the file is `foo.rs` or `foo/mod.rs`, write `#[cfg(test)] mod tests;` and put the tests in `foo/tests.rs`
  - Use the standard layout that resolves module paths without `#[path]`
- Put tests that use the code from outside (integration tests) in `crates/*/tests/`

### `mod.rs`

- Make `mod.rs` the table of contents of its module and the home of what the module shares
  - What goes there: `mod` declarations, re-exports, types shared within the module (errors, result types, traits), the module's main struct and its constructor, and small private functions they use
  - When it exceeds 200 lines excluding tests, move the logic of specific features into child modules. Keep the shared types and traits in `mod.rs`
  - The behavior of the main struct may be split into `impl` blocks in child modules (for example, `crates/feld-daemon/src/control/`)
- Give child modules names that show their role
