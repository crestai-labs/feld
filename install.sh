#!/bin/sh
# Install Feld: `feld` and the `feld-plugin-*` executables that must sit beside it.
#
#   curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
#
# Downloads the archive of a GitHub Release for this machine, checks it against its
# .sha256, and puts the executables in one directory. Supported: macOS on Apple Silicon
# and Linux on x86_64 (glibc 2.35 or later, e.g. Ubuntu 22.04 and Debian 12).
#
# Variables:
#   FELD_VERSION       the tag to install, e.g. v0.9.0 (default: the latest Release)
#   FELD_INSTALL_DIR   where the executables go (default: ~/.local/bin)
#   FELD_REPO          the GitHub repository of the Releases, owner/name
#                      (default: crestai-labs/feld)
#   GITHUB_TOKEN       a token that can read FELD_REPO; with it the archive comes through
#                      the GitHub API, which a private repository requires
set -eu

repo=${FELD_REPO:crestai-labs/feld}
dir=${FELD_INSTALL_DIR:-$HOME/.local/bin}
[ "$dir" = / ] || dir=${dir%/}
version=${FELD_VERSION:-}
token=${GITHUB_TOKEN:-}

say() { printf '%s\n' "$*"; }
fail() {
  printf 'install.sh: %s\n' "$1" >&2
  shift
  for line in "$@"; do printf '  %s\n' "$line" >&2; done
  exit 1
}

case $repo in
  */*) ;;
  *) fail "FELD_REPO must be owner/name: $repo" ;;
esac
command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"
if command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
elif command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d ' ' -f 1; }
else
  fail "shasum or sha256sum is required to check the download"
fi

os=$(uname -s)
arch=$(uname -m)
case $os/$arch in
  Darwin/arm64 | Darwin/aarch64) target=aarch64-apple-darwin ;;
  Darwin/x86_64) fail "Feld supports Apple Silicon Macs only; this Mac is Intel (x86_64)." ;;
  Linux/x86_64 | Linux/amd64) target=x86_64-unknown-linux-gnu ;;
  Linux/*) fail "Feld does not support Linux on $arch; Linux is supported on x86_64 only." ;;
  MINGW* | MSYS* | CYGWIN* | Windows*) fail "Feld does not support Windows." ;;
  *) fail "Feld does not support $os on $arch." ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' HUP INT TERM

# curl [options] URL: fetches, with the token as a header when there is one. The header
# goes through a config on stdin so that the token stays out of the process list.
# Prints the HTTP status, or 000 when the request did not reach a server.
fetch() {
  if [ -n "$token" ]; then
    printf 'header = "Authorization: Bearer %s"\n' "$token" |
      curl -sSL -K - -w '%{http_code}' "$@" 2>"$tmp/curl.err" || true
  else
    curl -sSL -w '%{http_code}' "$@" 2>"$tmp/curl.err" || true
  fi
}

# Stops on a status that is not 200, naming what was fetched.
check() {
  status=$1
  what=$2
  case $status in
    200) ;;
    000) fail "could not reach GitHub to fetch $what: $(cat "$tmp/curl.err")" \
      "Check the network connection and try again." ;;
    401) fail "GitHub rejected GITHUB_TOKEN while fetching $what (HTTP 401)." \
      "Set GITHUB_TOKEN to a valid token, e.g. GITHUB_TOKEN=\$(gh auth token)." ;;
    403 | 429) fail "GitHub refused to serve $what (HTTP $status), possibly a rate limit." \
      "Wait and try again, or set GITHUB_TOKEN to raise the limit." ;;
    404)
      if [ -n "$token" ]; then
        fail "$what was not found in $repo (HTTP 404)." \
          "Check FELD_REPO and FELD_VERSION, and that GITHUB_TOKEN can read $repo."
      else
        fail "$what was not found in $repo (HTTP 404)." \
          "Check FELD_REPO and FELD_VERSION. If $repo is private, set GITHUB_TOKEN, e.g. GITHUB_TOKEN=\$(gh auth token)."
      fi ;;
    *) fail "fetching $what failed with HTTP $status." "Try again later." ;;
  esac
}

# The archive is missing from a Release that exists: say whether that is the platform.
no_archive() {
  case $target in
    *linux*) fail "the Release $version of $repo has no archive for Linux yet ($target)." \
      "Feld is not yet released for Linux; build it from source meanwhile." ;;
    *) fail "the Release $version of $repo has no archive $1." \
      "Pick another version with FELD_VERSION, or try again once the Release carries it." ;;
  esac
}

api=https://api.github.com/repos/$repo
if [ -n "$version" ]; then
  case $version in v*) ;; *) version=v$version ;; esac
fi

if [ -n "$token" ]; then
  # A private repository serves its assets only through the API, by the id of each asset.
  if [ -n "$version" ]; then
    release_url=$api/releases/tags/$version
    what="the Release $version"
  else
    release_url=$api/releases/latest
    what="the latest Release"
  fi
  status=$(fetch -H 'Accept: application/vnd.github+json' -o "$tmp/release.json" "$release_url")
  check "$status" "$what"
  # One "key": value per line, whether the JSON is indented or not.
  tr ',{}' '[\n*]' <"$tmp/release.json" | sed 's/^[[:space:]]*//; s/^\("[^"]*"\): */\1: /' >"$tmp/release.lines"
  version=$(sed -n 's/^"tag_name": *"\([^"]*\)".*/\1/p' "$tmp/release.lines" | head -n 1)
  [ -n "$version" ] || fail "could not read the tag of $what from the GitHub API."
  name=feld-$version-$target
  # Each asset lists its API url before its name.
  asset_url() {
    awk -v want="\"name\": \"$1\"" '
      /^"url": "https:\/\/api\.github\.com\/.*\/releases\/assets\// { url = $2; gsub(/"/, "", url) }
      { line = $0; sub(/: +/, ": ", line) }
      line == want && url != "" { print url; exit }
    ' "$tmp/release.lines"
  }
  archive_url=$(asset_url "$name.tar.gz")
  sum_url=$(asset_url "$name.tar.gz.sha256")
  [ -n "$archive_url" ] || no_archive "$name.tar.gz"
  [ -n "$sum_url" ] || fail "the Release $version of $repo has no $name.tar.gz.sha256 to check the archive against."
  accept='Accept: application/octet-stream'
else
  if [ -z "$version" ]; then
    # github.com redirects the latest Release to its tag.
    latest=$(curl -sSL -o /dev/null -w '%{http_code} %{url_effective}' "https://github.com/$repo/releases/latest" 2>"$tmp/curl.err" || true)
    check "${latest%% *}" "the latest Release"
    version=${latest##*/}
    case $version in
      v*) ;;
      *) fail "$repo has no Release yet." "If $repo is private, set GITHUB_TOKEN, e.g. GITHUB_TOKEN=\$(gh auth token)." ;;
    esac
  fi
  name=feld-$version-$target
  archive_url=https://github.com/$repo/releases/download/$version/$name.tar.gz
  sum_url=$archive_url.sha256
  accept='Accept: */*'
fi

say "Installing Feld $version ($target) from $repo"

status=$(fetch -H "$accept" -o "$tmp/$name.tar.gz.sha256" "$sum_url")
if [ "$status" = 404 ] && [ -z "$token" ]; then
  # The tag may be missing too: tell the two apart.
  release=$(curl -sSL -o /dev/null -w '%{http_code}' "https://github.com/$repo/releases/tag/$version" 2>"$tmp/curl.err" || true)
  check "$release" "the Release $version"
  no_archive "$name.tar.gz"
fi
check "$status" "$name.tar.gz.sha256"
status=$(fetch -H "$accept" -o "$tmp/$name.tar.gz" "$archive_url")
check "$status" "$name.tar.gz"

expected=$(cut -d ' ' -f 1 <"$tmp/$name.tar.gz.sha256")
actual=$(sha256 "$tmp/$name.tar.gz")
[ -n "$expected" ] && [ "$expected" = "$actual" ] ||
  fail "the checksum of $name.tar.gz does not match its .sha256; nothing was installed." \
    "expected $expected" "got      $actual" \
    "The download may be corrupt: try again, and report it if it keeps failing."

tar -xzf "$tmp/$name.tar.gz" -C "$tmp" || fail "could not extract $name.tar.gz; nothing was installed."
[ -f "$tmp/$name/feld" ] || fail "$name.tar.gz has no feld executable; nothing was installed."

mkdir -p "$dir" || fail "could not create $dir." "Set FELD_INSTALL_DIR to a directory you can write."
# Each executable is renamed into place, so that a running one is replaced rather than rewritten.
for bin in "$tmp/$name/feld" "$tmp/$name"/feld-plugin-*; do
  [ -f "$bin" ] || continue
  base=${bin##*/}
  if ! { cp "$bin" "$dir/.$base.tmp" && chmod 755 "$dir/.$base.tmp" && mv -f "$dir/.$base.tmp" "$dir/$base"; }; then
    fail "could not write $dir/$base." "Set FELD_INSTALL_DIR to a directory you can write."
  fi
done

say "Installed Feld $version in $dir"
case :$PATH: in
  *:"$dir":*) ;;
  *)
    shell=${SHELL:-}
    case ${shell##*/} in
      zsh) rc=~/.zshrc ;;
      bash) rc=~/.bashrc ;;
      *) rc=~/.profile ;;
    esac
    say ""
    say "$dir is not on your PATH. Add it with:"
    say "  echo 'export PATH=\"$dir:\$PATH\"' >> $rc && export PATH=\"$dir:\$PATH\""
    ;;
esac
say ""
say "Next: feld init"
