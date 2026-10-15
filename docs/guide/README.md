# Guide

This directory holds the documents for the surfaces through which people and programs touch Feld (Quickstart, CLI, daemon API, Web UI, compatibility, Data & Privacy).

## Source of truth

- [`../spec/`](../spec/) is the source of truth for the specification Feld satisfies.
- The source of truth for the commands, arguments, default values, and results described in this directory is the current source code and executables.

When the implementation changes, update the relevant documents to match the actual CLI and behavior. When the specification and the implementation differ, fix the specification or the implementation, then reflect it in the documents in this directory.

## Documents

- [Quickstart](quickstart.md): install Feld, place Plugins with `feld init`, and try attaching and starting Claude Code on tmux, Messages, Jobs, and the Web UI in about 10 minutes. Also available in [Japanese](quickstart.ja.md) and [Simplified Chinese](quickstart.zh-CN.md).
- [Web UI views](webui.md): what the Console, Actor card, Timeline, Now, and Metrics lane draw.
- [CLI reference](cli.md): the arguments, options, behavior, and output of each `feld` command.
- [daemon API](daemon-api.md): the rules common to the `feld daemon` HTTP API (authentication, errors, name grammar, soft delete, time ranges, cursors, derived values, SSE).
- [daemon API OpenAPI definition](daemon-api.openapi.yaml): the request and response of each API. The `feld-daemon` tests check it against the routes, and the Web UI types are generated from it.
- [Compatibility](compatibility.md): how the store, `FELD_HOME`, CLI, API, and Plugin protocol are handled when upgrading during 0.x, and the steps to upgrade.
- [Data & Privacy](data-and-privacy.md): what Feld does not send outside, what it reads and writes on the user's machine, and how to remove it.
