# Web UI

The implementation of the Web UI that the daemon serves at `/ui/`. The rules for reading from and writing to the daemon are in [`docs/spec/design/webui.md`](../docs/spec/design/webui.md), and the view definitions are in [`docs/guide/webui.md`](../docs/guide/webui.md). The source of truth for the screens is the implementation and its tests.

## Tech stack

| Area | Details |
|---|---|
| Language | TypeScript (`strict`, `erasableSyntaxOnly`) and TSX. Styling is plain CSS |
| Rendering | React. Each page renders a host element (such as `<feld-agents>`), and the stylesheets in `styles/` select by class name |
| Routing | react-router in declarative mode. The URL holds the page, `?workspace=`, and the Session open in the Console (`?session=`) |
| Server state | TanStack Query. The daemon reads drawn on screen are held in queries and invalidated by notifications from `/v1/events`. The Console reads with a cursor-based `useInfiniteQuery`. The pairing screen checks the browser token with a direct `fetch` |
| UI state | State shared by multiple components lives in zustand (the Actors and Console `store.ts`, authentication, the event stream connection, the theme, viewer settings); local state uses `useState` |
| Build | Vite. `tsc` checks types and `vite build` writes to `dist/`. Scripts and stylesheets have fixed names; images have names with a hash of their content. `make check` checks the rebuilt `dist/` against the commit |
| Serving | The daemon's `web` module embeds all of `dist/` into the daemon with `rust-embed` and returns `index.html` for page routes. Every file is served with `Cache-Control: no-cache` and `ETag` so the browser revalidates it each time |
| Tests | vitest imports modules directly and checks them (`**/*.test.{js,ts,tsx}`) |

`dist/` is the build output committed as is, so `cargo build` and releases are complete with the committed `dist/`.
When you change the source, rebuild `dist/` with `make web` and include it in the same commit.

UI strings are in `shell/i18n.ts`, which holds no key missing from English.

## Layout

```text
web/
├── index.html / main.tsx   Entry point. QueryClient, router, event stream, page routes
├── api/                    daemon API
│   ├── client.ts           Requests, writes, one-off reads for the Console and Daemon
│   ├── auth.ts             Pairing and the browser token key; switches to the Sign in screen on 401
│   ├── queries.ts          Query keys, reads over the Actors range (useRangeData)
│   └── events.ts           Subscription to GET /v1/events and invalidation
├── shell/                  Sidebar, theme, viewer preferences (localStorage) and per-language strings, the Workspace query
├── actors/                 Actors page (model.ts derives, snapshot.ts maps daemon responses, store.ts holds UI state, charts/ draws SVG,
│                           Inspector.tsx is the cards overlaid on the Timeline: Console, Actor card, Message composer)
├── console/                The Session Console (transcript, output, Feld records). One opens in a card overlaid on the Actors Timeline
├── daemon/ / settings/     Daemon reads, restart and stop, the Settings page
├── fixtures/               Fixture mode (dev server only)
├── lib/                    Rules for transcript entries, quota lanes, argv, and composing range reads
├── types/                  Types of daemon responses (generated from daemon-api.openapi.yaml; feld-api.d.ts names them FeldApi) and shared types
├── tools/                  Type generator (openapi-typescript; a separate npm workspace because it uses the TypeScript 5 API)
├── styles/ / assets/       Stylesheets and logos
└── dist/                   Vite output (embedded by the daemon)
```

## Fixture mode

Fixture mode reads `fixtures/data/*.js` and draws fixtures in place of the daemon. It is a dev server only feature, and
`dist/` holds only the daemon reads. Open it with `?scenario=` (Actors: `three-days-ago-1h` / `three-days-ago-1d` / `message-density-30m`) and `?state=` (the exchange with the daemon: for Actors,
`connecting` / `reconnecting` / `error` / `refusing` together with `?scenario=`; for Settings, `running` / `degraded` /
`unreachable` / `reading` / `refusing`; for both, `unauthorized` / `expired` show the pairing screen). The clock and timezone follow the fixture.
The pairing screen in fixture mode accepts the fixture code (`K7QM-4XPD`); open it at `/ui/pair?scenario=three-days-ago-1h#code=K7QM-4XPD`.

`fixtures/data/session-*.js` are generated from made-up conversations in the shape of Claude Code transcript files (the user is `alice` and the working directory is under `/Users/alice/Dev/`). Do not edit them by hand.

```console
node web/fixtures/build-session-recording.mjs <source.jsonl> <output.js> [session-key]
```

The script removes private keys and replaces base64 images with a 1×1 PNG. It stops on a JSONL that still contains a home directory other than `/Users/alice`, an email address outside `example.com`, or a `claude.ai` URL. The other `fixtures/data/*.js` files are handwritten.

## Build and verification

```console
make web                                  # npm ci (only when the lockfile changed), tsc, vite build
make web-dist                             # after web, checks the diff between dist/ and the index, and untracked files
make test                                 # web, vitest, cargo test
make web-types                            # regenerates types/feld-api.generated.d.ts from the yaml and checks the diff against the commit
FELD_API=http://127.0.0.1:<port> npm --prefix web run dev   # dev server; /v1 is proxied to the daemon at FELD_API
```

On the dev server, pair with a real daemon at `/ui/pair#code=<code from feld ui pair>`, and open a fixture at `/ui/?scenario=three-days-ago-1h`.
