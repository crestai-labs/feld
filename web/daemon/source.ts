import { createContext, useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { readDaemonStatus, restartDaemon, shutdownDaemon } from "../api/client.ts";
import { showPairing, unpair } from "../api/auth.ts";
import { keys, useWorkspaces } from "../api/queries.ts";
import { subscribeDaemonEvents } from "../api/events.ts";
import { DEFAULT_WORKSPACE_KEY } from "../actors/snapshot.ts";
import { phaseOf, readIntervalOf, restartingOf, RESTART_READ_INTERVAL, type Phase, type Restart } from "./model.ts";

// Several notifications arriving together are one reason to read.
const NOTIFICATION_DELAY = 250;

export type DaemonAction = "restart" | "stop";

// What the Settings page draws of Feld's own configuration. The live source fills it from
// the daemon; a fixture fills it from its data on its clock.
export interface DaemonRead {
  phase: Phase;
  // The last status the daemon answered, kept while it is unreachable.
  status: FeldApi.DaemonStatus | null;
  // `serverTime` of the last read that succeeded.
  lastRead: string | null;
  // The Workspace keys, null until read.
  workspaces: string[] | null;
  defaultWorkspace: string;
  // A restart or stop the daemon refused.
  actionError: { action: DaemonAction; message: string } | null;
  // The restart this page asked for and the daemon accepted.
  restartedAt: number | null;
  now: number;
}

export interface DaemonSource extends DaemonRead {
  retry(): void;
  // Each settles once the daemon accepted or refused; a refusal shows as `actionError`.
  restart(): Promise<void>;
  stop(): Promise<void>;
  // Removes this browser's pairing and gives the page over to the pairing screen; throws
  // what the daemon answered when it refused.
  removeDevice(): Promise<void>;
}

// A fixture adapter provides a source here; without one the page reads the daemon.
export const DaemonSourceContext = createContext<DaemonSource | null>(null);

export function useLiveDaemonSource(): DaemonSource {
  const client = useQueryClient();
  // The moment the page learned the daemon is on its way out.
  const [goneAt, setGoneAt] = useState(0);
  const [actionError, setActionError] = useState<{ action: DaemonAction; message: string; at: number } | null>(null);
  const [lastRestart, setLastRestart] = useState<Restart | null>(null);
  const notificationTimer = useRef<number | null>(null);
  const workspaces = useWorkspaces();

  // The same query as the daemon's time zone and Nodes (see api/queries.ts). The interval
  // is this page's own; a read the query starts by itself follows the options its
  // observers last gave it.
  const query = useQuery({
    queryKey: keys.daemonStatus(),
    queryFn: ({ signal }) => readDaemonStatus(signal),
    // The interval is the page's only retry. It starts again after every read, as the
    // next read is scheduled from the last one.
    retry: false,
    refetchInterval: (current) => (restartingOf(lastRestart, current.state.data || null, Date.now()) ? RESTART_READ_INTERVAL : readIntervalOf(phaseOf(current.state, goneAt))),
    refetchIntervalInBackground: true,
  });

  const restarting = restartingOf(lastRestart, query.data || null, Date.now());
  const phase = restarting ? "reachable" : phaseOf(query, goneAt);

  // The newest phase is the one the page keeps: a read still in flight and a read a
  // notification asked for are both dropped, leaving the retry as the only read ahead.
  const setGone = useCallback(() => {
    window.clearTimeout(notificationTimer.current!);
    notificationTimer.current = null;
    client.cancelQueries({ queryKey: keys.daemonStatus() });
    setGoneAt(Date.now());
  }, [client]);

  // The daemon's own notifications: one of them means the status may have changed, and
  // the shutdown notification means the daemon is on its way out. The stream opens by
  // naming the daemon instance; the shutdown notification is the one `daemon`
  // notification that says something the page has not read already. A stream that
  // reconnects reads every query again (see api/events.ts).
  useEffect(() => {
    const unsubscribe = subscribeDaemonEvents((event) => {
      if (event.resource === "daemon") {
        if (event.id === "shutdown") setGone();
        return;
      }
      window.clearTimeout(notificationTimer.current!);
      notificationTimer.current = window.setTimeout(() => {
        notificationTimer.current = null;
        client.invalidateQueries({ queryKey: keys.daemonStatus() });
      }, NOTIFICATION_DELAY);
    });
    return () => {
      unsubscribe();
      window.clearTimeout(notificationTimer.current!);
    };
  }, [client, setGone]);

  const { refetch } = query;
  const retry = useCallback(() => {
    refetch();
  }, [refetch]);

  // The daemon answers before it goes; it closes the event stream as it goes.
  const status = query.data;
  const act = useCallback(async (action: DaemonAction) => {
    setActionError(null);
    try {
      await (action === "restart" ? restartDaemon() : shutdownDaemon());
    } catch (error) {
      setActionError({ action, message: (error as Error).message, at: Date.now() });
      return;
    }
    // A restart that outlasts its limit leaves the daemon unreachable from the moment it
    // was accepted.
    setLastRestart(action === "restart" ? { at: Date.now(), instanceId: status!.instanceId } : null);
    setGone();
  }, [setGone, status]);
  const restart = useCallback(() => act("restart"), [act]);
  const stop = useCallback(() => act("stop"), [act]);

  return {
    phase,
    // A restart under way starts the process again at the restart.
    status: restarting ? { ...query.data!, startedAt: new Date(lastRestart!.at).toISOString() } : query.data || null,
    lastRead: query.data?.serverTime || null,
    workspaces: workspaces.data || null,
    defaultWorkspace: DEFAULT_WORKSPACE_KEY,
    // A read that succeeds after a refused action clears the notice.
    actionError: actionError && actionError.at > query.dataUpdatedAt ? { action: actionError.action, message: actionError.message } : null,
    restartedAt: lastRestart?.at ?? null,
    now: Date.now(),
    retry,
    restart,
    stop,
    removeDevice,
  };
}

// POST /v1/ui/unpair revokes this browser's token and deletes its cookie. The preferences
// stay in this browser.
async function removeDevice() {
  await unpair();
  showPairing("removed");
}
