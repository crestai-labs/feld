import { useEffect } from "react";
import { useQueryClient, type QueryClient, type QueryKey } from "@tanstack/react-query";
import { create } from "zustand";
import { keyHeaders, noteUnauthorized } from "./auth.ts";
import { isRangeKey, keys } from "./queries.ts";
import { invalidationCollapser } from "../lib/invalidations.ts";

// `GET /v1/events`: every notification names the resource that changed, and the queries
// that read that resource are invalidated. A burst of notifications is collapsed into one
// invalidation a second after the first of them.
const EVENT_DELAY = 1000;
const FIRST_RECONNECT_DELAY = 500;
const MAX_RECONNECT_DELAY = 10000;

// The stream's own state, which tells a page whether it follows the daemon live.
// `reconnecting` is a stream that was connected and lost its connection.
export const useEventStream = create<{ connected: boolean; reconnecting: boolean }>(() => ({ connected: false, reconnecting: false }));

// A paused Live range and a historical range, read on Refresh: a notification, the stream's
// reconnection and the browser's return online mark the range's reads stale without reading
// them again.
let rangeHeld = false;

export function holdRangeReads(held: boolean) {
  rangeHeld = held;
}

export function holdsRead(queryKey: QueryKey) {
  return rangeHeld && isRangeKey(queryKey);
}

type Listener = (event: FeldApi.Event) => void;
const listeners = new Set<Listener>();

// Every notification, for a page that reacts to one itself (the Console follows its
// Session's log, the Daemon page lists the stream).
export function subscribeDaemonEvents(listener: Listener) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// The Actor listings that list a Session, which its notification makes stale as well.
export function actorListingsHolding(client: QueryClient, event: FeldApi.Event): QueryKey[] {
  if (event.resource !== "session") return [];
  return client
    .getQueryCache()
    .findAll({ queryKey: keys.actorListingsAll() })
    .filter((query) => (query.state.data as { sessions?: { id: string }[] } | undefined)?.sessions?.some((session) => session.id === event.id))
    .map((query) => query.queryKey);
}

// The queries one notification makes stale.
export function invalidatedBy(event: FeldApi.Event): QueryKey[] {
  switch (event.resource) {
    case "workspace":
      return [keys.workspaces(), keys.actorsAll()];
    // An Actor's notification comes with each Session it starts.
    case "actor":
      return [keys.actorsAll(), keys.actorListingsOf(event.id)];
    // A Session the page holds is read again with its metrics and its Sub Agent Sessions';
    // one it does not hold may have entered the range, which the listing tells. The
    // Runtime metrics and quota follow on their own period (see useRangeData).
    case "session":
      return [keys.session(event.id), keys.metricsOf(event.id), keys.listingAll()];
    // A transcript that grew may show a new Sub Agent.
    case "log":
      return [keys.session(event.id)];
    case "record":
      return [keys.recordsAll()];
    // A Message changes its addressee's waiting count as well.
    case "message":
      return [keys.messagesAll(), keys.actorsAll()];
    default:
      return [];
  }
}

function framesOf(buffer: string) {
  const frames: string[] = [];
  let rest = buffer;
  let boundary: number;
  while ((boundary = rest.indexOf("\n\n")) >= 0) {
    frames.push(rest.slice(0, boundary));
    rest = rest.slice(boundary + 2);
  }
  return { frames, rest };
}

function notificationOf(frame: string): FeldApi.Event | null {
  const data = frame
    .split("\n")
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice(5).trim())
    .join("\n");
  if (!data) return null;
  try {
    return JSON.parse(data);
  } catch (_error) {
    // A frame that is not the daemon's JSON names nothing.
    return null;
  }
}

// What a stream that reconnects may have missed: every query but a held one is read again,
// and a held one is marked stale.
export function invalidateMissed(client: QueryClient) {
  client.invalidateQueries({ predicate: (query) => !holdsRead(query.queryKey) });
  client.invalidateQueries({ predicate: (query) => holdsRead(query.queryKey), refetchType: "none" });
}

// What a write the daemon accepted changed, named by the write. As with a notification, a
// read in flight runs to its end and a held query is only marked stale. The read in flight
// may have left before the write, and its answer clears the stale mark, so its query is
// invalidated again once it ends. The write does not wait for the reads.
export function invalidateWritten(client: QueryClient, queryKeys: QueryKey[], exact = false) {
  for (const queryKey of queryKeys) {
    // Which reads are in flight is taken before the invalidation starts a read of its own.
    const inFlight = client.getQueryCache().findAll({ queryKey, exact, fetchStatus: "fetching" });
    if (holdsRead(queryKey)) client.invalidateQueries({ queryKey, exact, refetchType: "none" });
    else client.invalidateQueries({ queryKey, exact }, { cancelRefetch: false }).catch(() => {});
    for (const query of inFlight) {
      query.promise?.catch(() => {}).then(() => invalidateWritten(client, [query.queryKey], true));
    }
  }
}

// Makes stale what one notification changed. A `resync` tells that the stream fell behind
// and dropped notifications, which leaves the page as a reconnection does.
export function noteNotification(client: QueryClient, collapse: ReturnType<typeof invalidationCollapser>, event: FeldApi.Event) {
  if (event.resource === "resync") invalidateMissed(client);
  else collapse.add([...invalidatedBy(event), ...actorListingsHolding(client, event)]);
}

// Opens the stream for the page's lifetime, scoped to one Workspace when one is shown
// (undefined opens none).
export function useDaemonEvents(workspace: string | null | undefined) {
  const client = useQueryClient();
  useEffect(() => {
    if (workspace === undefined) return;
    const controller = new AbortController();
    const collapse = invalidationCollapser(client, EVENT_DELAY, holdsRead);
    (async () => {
      let delay = FIRST_RECONNECT_DELAY;
      let reconnecting = false;
      while (!controller.signal.aborted) {
        try {
          const query = workspace ? `?workspace=${encodeURIComponent(workspace)}` : "";
          const response = await fetch(`/v1/events${query}`, {
            headers: { Accept: "text/event-stream", ...keyHeaders() },
            cache: "no-store",
            signal: controller.signal,
          });
          // A rejected browser token stays rejected until the browser pairs again.
          if (response.status === 401) {
            useEventStream.setState({ connected: false, reconnecting: false });
            noteUnauthorized();
            break;
          }
          if (!response.ok || !response.body) throw new Error(`events ${response.status}`);
          delay = FIRST_RECONNECT_DELAY;
          useEventStream.setState({ connected: true, reconnecting: false });
          if (reconnecting) invalidateMissed(client);
          const reader = response.body.getReader();
          const decoder = new TextDecoder();
          let buffer = "";
          for (;;) {
            const { value, done } = await reader.read();
            if (done) throw new Error("the event stream closed");
            const { frames, rest } = framesOf(buffer + decoder.decode(value, { stream: true }).replace(/\r\n/g, "\n"));
            buffer = rest;
            for (const frame of frames) {
              const event = notificationOf(frame);
              if (!event) continue;
              noteNotification(client, collapse, event);
              for (const listener of listeners) listener(event);
            }
          }
        } catch (_error) {
          if (controller.signal.aborted) break;
          reconnecting = true;
          useEventStream.setState({ connected: false, reconnecting: true });
          await new Promise((resolve) => window.setTimeout(resolve, delay));
          delay = Math.min(delay * 2, MAX_RECONNECT_DELAY);
        }
      }
    })();
    return () => {
      controller.abort();
      collapse.cancel();
      useEventStream.setState({ connected: false, reconnecting: false });
    };
  }, [client, workspace]);
}
