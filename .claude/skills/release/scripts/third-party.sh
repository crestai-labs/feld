#!/bin/sh
# Write the list of the third-party software the released binaries bundle, with the
# license of each: the crates the binaries of the workspace link, and the packages web/dist is built from.
# Usage: third-party.sh <output file> [target triple]
# A target triple narrows the crates to those the binaries of that target link; without one,
# the list covers every target of `about.toml`.
# Needs cargo-about (cargo install cargo-about --locked --features cli) and web/node_modules.
set -eu

out=${1:?usage: third-party.sh <output file> [target triple]}
target=${2:-}
root=$(git rev-parse --show-toplevel)
cd "$root"
here=$(cd "$(dirname "$0")" && pwd)

command -v cargo-about >/dev/null || { echo "cargo-about is not installed" >&2; exit 2; }
[ -d web/node_modules ] || npm --prefix web ci --silent

{
  echo "# Third-party software"
  echo
  echo "Feld's binaries bundle the software below. Each is under the license shown with it."
  echo
  echo "## Rust crates"
  echo
  cargo about generate --locked --workspace ${target:+--target "$target"} about.hbs
  echo "## Web UI packages"
  echo
  node "$here/web-licenses.mjs" web
} > "$out"
