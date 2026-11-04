#!/bin/sh
# Runs the binaries `cargo test` and `cargo run` build. tmux puts a named server's socket at
# $TMUX_TMPDIR/tmux-<uid>/<name>, and a Unix socket path holds at most 104 bytes on macOS. In a
# checkout whose target/ leaves too little room for that, the servers go to a directory of
# its own under /tmp instead, named after the target/ it stands for.
if [ "${#TMUX_TMPDIR}" -gt 60 ]; then
    TMUX_TMPDIR="/tmp/feld-tmux-$(printf %s "$TMUX_TMPDIR" | cksum | cut -d ' ' -f 1)"
    mkdir -p "$TMUX_TMPDIR" || exit 1
    export TMUX_TMPDIR
fi
exec "$@"
