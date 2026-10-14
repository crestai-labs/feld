#!/bin/sh
# Build the release binaries and pack them, with the licenses, into one archive with its sha256.
# Usage: package.sh vX.Y.Z
# Packs the repository the working directory is in.
# Writes dist/feld-vX.Y.Z-<host triple>.tar.gz and .tar.gz.sha256, and prints their paths.
set -eu

tag=${1:?usage: package.sh vX.Y.Z}
case $tag in v[0-9]*) ;; *) echo "tag must look like vX.Y.Z: $tag" >&2; exit 2 ;; esac

here=$(cd "$(dirname "$0")" && pwd)
root=$(git rev-parse --show-toplevel)
cd "$root"

target=$(rustc -vV | sed -n 's/^host: //p')
name="feld-$tag-$target"

# rustc writes the paths of the sources into the binaries, for panic messages. They name
# the home directory of whoever builds, so they are rewritten: the last match applies.
cargo_home=${CARGO_HOME:-$HOME/.cargo}
RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/home --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$root=/feld" \
  make release

# Every bin target of the workspace goes in: feld and the Plugin executables that must sit beside it.
bins=$(cargo metadata --no-deps --format-version 1 | python3 -c '
import json, sys
meta = json.load(sys.stdin)
for package in meta["packages"]:
    for target in package["targets"]:
        if "bin" in target["kind"]:
            print(target["name"])
')

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name" dist
for bin in $bins; do
  if LC_ALL=C grep -q -a -F "$HOME" "target/release/$bin"; then
    echo "target/release/$bin names $HOME" >&2
    exit 1
  fi
  cp "target/release/$bin" "$stage/$name/"
done

# The license of Feld and of what the binaries bundle travel with them.
cp LICENSE NOTICE "$stage/$name/"
"$here/third-party.sh" "$stage/$name/THIRD-PARTY-LICENSES.md" "$target"

tar -C "$stage" -czf "dist/$name.tar.gz" "$name"
# `<sha256>  <name>`, as both tools write it and install.sh reads it.
if command -v shasum >/dev/null 2>&1; then
  (cd dist && shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256")
else
  (cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
fi

echo "dist/$name.tar.gz"
echo "dist/$name.tar.gz.sha256"
