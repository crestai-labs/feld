.PHONY: build release web web-dist web-types test check
# Every crate of the workspace: `feld` and the `feld-plugin-<name>` executables that
# `feld init` finds beside it.
build:
	cargo build --workspace

# The same set, optimized, into target/release, with the dependencies the lockfile pins.
release:
	cargo build --release --locked --workspace

# The Web UI is type-checked and built by Vite into web/dist, which the daemon embeds
# and the repository carries, so `build` needs no Node. The dependencies are installed
# again only when the lockfile changes.
web: web/node_modules/.package-lock.json
	npm --prefix web run --silent build

# The committed web/dist is the build of the committed source, so a source that changes
# without it, or a build that changes or adds a file in it, fails here.
web-dist: web
	git diff --exit-code -- web/dist
	! git ls-files --others --exclude-standard -- web/dist | grep .

# The Web UI's types of the daemon API are generated from the OpenAPI definition and
# committed, so a definition that changes without them fails here.
web-types: web/node_modules/.package-lock.json
	npm --prefix web run --silent types
	git diff --exit-code -- web/types/feld-api.generated.d.ts

web/node_modules/.package-lock.json: web/package-lock.json
	npm --prefix web ci --silent

# The E2E tests attach fake Claude Codes and start their own daemons, so the configuration
# root of a Claude Code this may run inside and the pane of a tmux or herdr this may run
# inside stay out of them. The fixtures keep the variables of a Feld Session out themselves.
# `cargo test` builds the binaries of the package under test alone, so `build` puts the
# Plugin executables beside `feld` first.
test: web build
	npm --prefix web run --silent test
	env -u CLAUDE_CONFIG_DIR -u CODEX_HOME -u CODEX_THREAD_ID -u CODEX_SESSION_ID -u TMUX -u TMUX_PANE -u HERDR_ENV -u HERDR_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_SESSION cargo test --workspace

check: web-types web-dist test
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
