# Web UI reads and writes

This document explains how the Web UI combines daemon reads, notifications from `GET /v1/events`, and writes to draw its screens. The API follows [daemon-api.md](../../guide/daemon-api.md), view definitions follow [guide/webui.md](../../guide/webui.md), and authentication follows [auth.md](auth.md).

## Contents

- [Pages and URLs](#pages-and-urls)
- [Reading a range](#reading-a-range)
- [Reading an Actor card](#reading-an-actor-card)
- [Notifications and rereads](#notifications-and-rereads)
- [Console](#console)
- [Writes](#writes)
- [Authentication](#authentication)
- [Viewer settings](#viewer-settings)

## Pages and URLs

The daemon serves the Actors page at `/ui/`, the Settings page at `/ui/settings`, and the pairing screen at `/ui/pair`. `/ui/daemon` redirects to the Daemon section of Settings (`/ui/settings#settings-daemon`). The URL holds the page, the Workspace (`?workspace=`), and the Session open in the Console (`?session=`), so back, forward, reload, and a shared URL all show the same screen.

## Reading a range

For the displayed range, the Actors page separately reads the Actor list per Workspace, `GET /v1/sessions`, `GET /v1/sessions/{sessionId}` and metrics per Session, the Timeline records, and Messages, and draws each as it arrives. It reads the Actor list first and draws the bands, then adds each Session's Sub Agent Sessions and metrics as they arrive. A Sub Agent Session is drawn when the part from the parent Session's start onward overlaps the range ([Timeline](../../guide/webui.md#timeline)).

When the Console draws a Message record, it reads that Message once with `GET /v1/messages/{message}` and shows the full body. Until that arrives, it shows the list's `preview`.

Per-Session reads and metrics run at most 3 at a time, leaving the rest of the browser's connections to the same host free for range reads. Even when per-Session reads are slow, Live rereads go out on schedule.

The Metrics lane reads `GET /v1/runtimes/{runtime}/metrics` for each installed Runtime over the Metrics window ([Metrics lane](metrics.md#metrics-lane)). In a Live range it reads at a period of 1/12 of the bin width (at least 5 seconds); in Paused and past ranges it reads on Refresh; while the range is moving it reads once the range stops.

A range whose end is now is called Live, and the toggle in the page header can switch it to Paused.

- A Live range rereads on notifications, and reads every 5 seconds while the event stream is down. The same applies to the tail of the current Session shown in an Actor's row, which follows along like the Console only while Live
- Paused and past ranges keep notifications as a stale mark and read on Refresh. Switching back to Live rereads the stale reads

When the range moves, the reads of the previous range stay drawn and are replaced once the reads of the new range arrive.

## Reading an Actor card

Regardless of the range, an Actor card lists all of the Actor's Sessions and Sub Agent Sessions with `GET /v1/sessions?workspace=&actor=&subAgents`, and keeps those reads while a Console opened from the card is open. The metrics of the selected Session are read over its lifetime. This list is reread on notifications for the Actor and for Sessions in the list. The Timeline draws only the Sessions in the range.

## Notifications and rereads

A page opens one `GET /v1/events` stream. The subscription scope is the displayed Workspace on the Actors page (default when `?workspace=` is absent) and all Workspaces on the Settings page; browser notifications use the same scope.

Reads of the resources a notification names are redone, batched over 1 second. A notification that arrives during a read lets that read complete without aborting it, and the read is redone once after it completes. On reconnect, all reads are redone. Failed reads are retried with a widening interval.

## Console

Transcript and output are read with a cursor. Reading starts at the tail, reads older pages with `before` and appended content with `after`. Appended content is read every second and on `log` notifications, and is followed only while Live.

## Writes

A write settles its menu or dialog when the daemon accepts it. It does not wait for rereads; only the reads of the resources the write changed are redone, treated the same as for a notification (an in-progress read completes without aborting and is redone once afterward; in Paused and past ranges, a stale mark).

## Authentication

Reads, writes, and `GET /v1/events` pass with the browser token's cookie and key ([cookie and key](auth.md#cookie-and-key)). The cookie is HttpOnly, so the page cannot tell whether it has one; when a request returns 401, the page switches to the Sign in screen, and reads and the event stream stop there. A 401 after a successful read shows "access has ended".

The pairing procedure follows [auth.md](auth.md#pairing). `/ui/pair#code=` shows only the logo, and every other page reloads itself after pairing. The daemon answers every failed code with 401, so the page counts 5 consecutive failures and shows "all codes are now invalid".

## Viewer settings

Language, time zone and format, theme, the Console's default view, notifications, and layout are kept in the browser's `localStorage`, and each page reads them when drawing. The default zone is the daemon's `timezone`.
