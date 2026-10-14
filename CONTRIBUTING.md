# Contributing to Feld

Contributions to Feld are welcome. This document covers how to start developing and how to file Issues and open Pull Requests.

## Development setup

Requirements:

- Rust toolchain: a version that can build edition 2024 (Rust 1.85 or later)
- Node.js (npm): used to build and test `web/`. Vite and vitest require an LTS version of 22.12 or later
- tmux, or [herdr](https://herdr.dev)
- Claude Code, Codex (optional; used to try Runtime Adapters against the real products)

Build from source.

```sh
git clone https://github.com/crestai-labs/feld
cd feld
make
export PATH="$PWD/target/debug:$PATH"
```

`make` builds every crate in the workspace (`feld` and the `feld-plugin-<name>` binaries that `feld init` looks for next to it) into `target/debug`. It embeds the committed `web/dist/`, so Node is not needed. Usage from there is in [`README.md`](README.md).

## Repository layout

### Crates

The repository root holds the Cargo workspace definition, and the crates are in `crates/`. `feld-core` and `feld-plugin-<name>` are described under [Plugins](#plugins).

| crate | Contents |
|---|---|
| `feld-cli` | The package and binary are named `feld`. Holds the CLI and a `build.rs` that embeds the version. The daemon also runs from this binary as `feld daemon` |
| `feld-daemon` | The daemon library. Holds `api`, `control`, `scheduler`, `service`, `store`, `metrics`, `console`, `daemon`, `collector` and `bundled`, and embeds the Web UI's `web/dist` |

Integration tests that run the `feld` binary, and their fixtures, are in `crates/feld-cli/tests/`. Tests that exercise the store directly are in `crates/feld-daemon/tests/`.

### Plugins

Runtime Adapters and Execution Providers are Plugins placed in `$FELD_HOME/plugins/<kind>/<name>/`, which the daemon reads at startup. The bundled ones are `claude-code`, `codex`, `tmux` and `herdr`, and `feld init` writes the selected ones there. The static files of the bundled Plugins are in the repository's `plugins/<kind>/<name>/`. Their executables are binaries of the Cargo workspace crates `crates/feld-plugin-<name>`, built together with `feld`. The product-independent skeleton is in `feld_core::plugin::sdk` and `feld_core::self_api` of `crates/feld-core`, the crate shared by the daemon, the CLI and Plugins. The only Feld crate a Plugin crate depends on is `feld-core`. The contract with core is in [`docs/spec/design/plugin.md`](docs/spec/design/plugin.md).

## Checking changes

Pass `make check` before opening a PR.

```sh
make web     # Type-check the Web UI and build web/dist, which the daemon embeds, with Vite
make test    # Build web and the workspace, then run the Web UI's vitest and cargo test for the whole workspace
make check   # Web UI API types match the OpenAPI definition, web/dist matches the source, test, cargo fmt --check, cargo clippy
```

When you change the Web UI source, rebuild `web/dist/` with `make web` and include it in the same commit ([`web/README.md`](web/README.md)). `make check` confirms that the rebuilt `web/dist/` matches the index.

### E2E tests and CI

The policy for tests that depend on real products (what runs against the real thing and what uses a substitute, what automated tests do not prove, what tests do not touch), and the steps to run the real Claude Code and Codex with `FELD_TEST_REQUIRE`, are in [`crates/feld-cli/tests/README.md`](crates/feld-cli/tests/README.md).

CI (`.github/workflows/ci.yml`) runs `cargo deny`, `npm --prefix web audit` and `make check` on Ubuntu. `npm audit` includes devDependencies and fails on any advisory of low or higher. A manual run runs two configurations, minimum and latest, in parallel.

| Trigger | Product versions | `FELD_TEST_REQUIRE` |
|---|---|---|
| Push to main (except changes to only `docs/**` and `**.md`) | Ubuntu 24.04, tmux from the OS, latest herdr | `tmux,herdr` |
| Manual run (`gh workflow run ci.yml --ref <branch>`), pre-release minimum configuration | Ubuntu 22.04, tmux from the OS (3.2a), herdr 0.9.0, Claude Code 2.1.181, Codex 0.156.0, Node 22 | `tmux,herdr,claude-code,codex` |
| Same manual run, pre-release latest configuration | Ubuntu 24.04, latest tmux (built from source), herdr, Claude Code and Codex | `tmux,herdr,claude-code,codex` |

The minimum versions are the defaults of the workflow inputs, and each product can be overridden with `gh workflow run ci.yml --ref <branch> -f herdr=<version> -f claude-code=<version> -f codex=<version>`. Manual runs use the repository secrets `ANTHROPIC_API_KEY` and `OPENAI_API_KEY`. So that Codex's sandbox (bubblewrap) can create user namespaces, the latest-configuration job sets AppArmor's `kernel.apparmor_restrict_unprivileged_userns` to 0. A new push to main cancels older runs. Each product's version appears in the run summary.

`.github/workflows/pr.yml` runs the checks that need no Runtime.

| Trigger | Checks |
|---|---|
| PR | DCO sign-off line, `cargo deny check advisories licenses`, `npm --prefix web audit`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` |
| Manual run (`gh workflow run pr.yml --ref <branch>`) | `cargo deny check advisories licenses`, `npm --prefix web audit`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` |
| Every Monday at 00:00 (UTC) | `cargo deny check advisories`, `npm --prefix web audit` |

## Filing issues

Create Issues from a template.

- **Bug report**: behavior that does not work as expected
- **Feature request**: a feature you want or a change in behavior
- **Proposal**: propose a change to the CLI, API, Web UI or design and discuss whether to adopt it. Discussion happens in the Issue comments, and once adopted, a PR updates `docs/spec`

Report vulnerabilities privately, following [`SECURITY.md`](SECURITY.md).

## Opening pull requests

- Add a DCO ([Developer Certificate of Origin](https://developercertificate.org/)) `Signed-off-by` line to every commit. `git commit -s` adds it. No CLA is required
  - The PR check (`dco` in `pr.yml`) confirms that every commit in the PR, except merge commits, has a sign-off line with the same email as the commit author, and fails listing the commits without one
  - The commit author adds the sign-off. `git commit -s` and `git rebase --signoff` add the sign-off of the person running them (the committer). To add a sign-off to commits you authored, run `git commit --amend -s --no-edit` for the last commit, or `git rebase --signoff origin/main` for every commit in the PR, then push again with `git push --force-with-lease`
- Write commit messages as `<target>(<type>): <message>`. `<target>` is the changed unit such as `cli`, `daemon`, `web` or `docs` (comma-separated if several), and `<type>` is a [Conventional Commits](https://www.conventionalcommits.org/) type such as `feat` or `fix`. Example: `docs(fix): fix README.md`
- Keep PR titles short, without a prefix (such as `docs:`). Link the Issue by writing `Closes #<issue>` in the body
- For changes that affect the spec, fix `docs/spec` first. The precedence of `docs/spec`, the implementation and `docs/guide` is in [`AGENTS.md`](AGENTS.md)
- Maintainers review PRs
- The repo has Dependabot alerts and Dependabot security updates enabled. PRs from security updates are reviewed before merge, like any other PR

## License

Contributions are provided under this repo's [Apache License 2.0](LICENSE).

## Developing with agents

Development with Coding Agents is welcome. The repo layout and documentation rules are in [`AGENTS.md`](AGENTS.md), so have your Agent read it.
