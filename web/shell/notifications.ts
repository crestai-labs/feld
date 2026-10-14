import { useEffect, useRef } from "react";
import { api, readDaemonStatus, readSession } from "../api/client.ts";
import { subscribeDaemonEvents } from "../api/events.ts";
import { instantText } from "../settings/parts.tsx";
import { t } from "./i18n.ts";
import * as prefs from "./preferences.ts";

// Browser notifications for the conditions the Settings page chose. Each daemon
// notification names what changed; the tab reads it and notifies what happened after the
// tab opened, on the daemon's clock. The tag names the Session, Message or record, so tabs that see the same
// change show one notification.

export type NotificationCondition = (typeof prefs.NOTIFICATIONS)[number];

export interface BrowserNotice {
  condition: NotificationCondition;
  title: string;
  body: string;
  tag: string;
}

// How long a Message stays waiting before it is notified.
export const MESSAGE_WAITING_DELAY = 60_000;
// A burst of notifications about one resource is read once.
const READ_DELAY = 1000;

const FAILED_OUTCOMES: FeldApi.Outcome[] = ["failed", "unknown"];

function afterOpen(at: string | null | undefined, since: number) {
  return Boolean(at) && Date.parse(at!) >= since;
}

// A Session that never began is a failed start; one that ended failed or unknown after the
// tab opened is a failed Session.
export function sessionNotice(session: FeldApi.Session, since: number): BrowserNotice | null {
  const vars = { actor: session.actor, session: session.id };
  if (session.lifecycle.startFailed) {
    if (!afterOpen(session.lifecycle.startFailed, since)) return null;
    const body = t("notice.start-failed.body", { ...vars, detail: session.observation?.detail || "" }).trim();
    return { condition: "start-failed", title: t("notice.start-failed", vars), body, tag: `feld:${session.id}` };
  }
  if (!afterOpen(session.end, since) || !FAILED_OUTCOMES.includes(session.outcome)) return null;
  return {
    condition: "session-failed",
    title: t("notice.session-failed", vars),
    body: t("notice.session-failed.body", { ...vars, outcome: session.outcome }),
    tag: `feld:${session.id}`,
  };
}

// A Message accepted after the tab opened that is still waiting a minute later.
export function messageNotice(message: FeldApi.MessageSummary, since: number, now: number): BrowserNotice | null {
  if (!message.waiting || message.deletedAt || !afterOpen(message.accepted, since)) return null;
  if (now - Date.parse(message.accepted) < MESSAGE_WAITING_DELAY) return null;
  const from = message.from.type === "actor" ? message.from.name : "Admin";
  return {
    condition: "message-waiting",
    title: t("notice.message-waiting", { actor: message.to.name }),
    body: t("notice.message-waiting.body", { from, preview: message.preview.trim() }),
    tag: `feld:${message.id}`,
  };
}

export function skipNotice(record: FeldApi.FeldRecord, zone: string): BrowserNotice {
  const data = record.data as { planned?: string; currentSessionId?: string };
  const planned = data.planned ? instantText(Date.parse(data.planned), zone) : "";
  return {
    condition: "schedule-skipped",
    title: t("notice.schedule-skipped", { actor: record.actor.name }),
    body: t("notice.schedule-skipped.body", { planned, session: data.currentSessionId || "" }),
    tag: `feld:record:${record.sequence}`,
  };
}

function wanted(condition: NotificationCondition) {
  return "Notification" in window && Notification.permission === "granted" && prefs.notifications().has(condition);
}

// Notifies while the page follows the daemon's events (see useDaemonEvents), over the
// same Workspace (null: every Workspace; undefined: no daemon).
export function useBrowserNotifications(workspace: string | null | undefined, daemonZone: string | null) {
  // The zone only writes the planned run of a skip; its arrival keeps the reads pending.
  const zone = useRef(daemonZone);
  zone.current = daemonZone;
  useEffect(() => {
    if (workspace === undefined || !("Notification" in window)) return;
    const opened = Date.now();
    // How far the daemon's clock is ahead of the browser's, from its `serverTime` against
    // the middle of the read; a failed read leaves it to the next use.
    let skew: Promise<number> | null = null;
    function daemonSkew() {
      if (!skew) {
        const before = Date.now();
        skew = readDaemonStatus()
          .then((status) => Date.parse(status.serverTime) - (before + Date.now()) / 2)
          .catch(() => {
            skew = null;
            return 0;
          });
      }
      return skew;
    }
    void daemonSkew();
    const shown = new Set<string>();
    const timers = new Map<string, number>();
    // Skips are read from `skipStart` (the browser's clock), then from the cursor; a skip
    // while the condition is off moves the start past it.
    let skipStart = opened;
    let skipCursor: number | null = null;

    function show(notice: BrowserNotice | null) {
      if (!notice || !wanted(notice.condition) || shown.has(notice.tag)) return;
      shown.add(notice.tag);
      const notification = new Notification(notice.title, { body: notice.body, tag: notice.tag });
      notification.onclick = () => {
        window.focus();
        notification.close();
      };
    }

    // One read per resource after `delay`; a later notification of it joins the pending read.
    function later(key: string, delay: number, read: () => Promise<void>) {
      if (timers.has(key)) return;
      timers.set(key, window.setTimeout(() => {
        timers.delete(key);
        read().catch(() => {
          // An unreadable resource notifies nothing; its next notification reads it again.
        });
      }, delay));
    }

    function readMessage(id: string) {
      return api<{ message: FeldApi.Message }>(`/v1/messages/${encodeURIComponent(id)}`).then((answer) => answer.message);
    }

    async function readSkips() {
      const query = new URLSearchParams({ kind: "schedule.skipped" });
      if (workspace) query.set("workspace", workspace);
      if (skipCursor === null) query.set("start", new Date(skipStart + (await daemonSkew())).toISOString());
      else query.set("after", String(skipCursor));
      const answer = await api<{ records: FeldApi.FeldRecord[]; nextAfter: number }>(`/v1/records?${query}`);
      // An empty read answers the `after` it was given, 0 for a read from `skipStart`.
      if (answer.records.length) skipCursor = answer.nextAfter;
      const shownZone = prefs.timeZone(zone.current);
      for (const record of answer.records) show(skipNotice(record, shownZone));
    }

    const unsubscribe = subscribeDaemonEvents((event) => {
      if (event.resource === "session" && (wanted("session-failed") || wanted("start-failed"))) {
        later(`session:${event.id}`, READ_DELAY, async () => {
          const { session } = await readSession(event.id);
          show(sessionNotice(session, opened + (await daemonSkew())));
        });
      } else if (event.resource === "message" && wanted("message-waiting")) {
        later(`message:${event.id}`, READ_DELAY, async () => {
          const message = await readMessage(event.id);
          const ahead = await daemonSkew();
          if (!message.waiting || message.deletedAt || !afterOpen(message.accepted, opened + ahead)) return;
          // Read again once the Message has waited its minute on the daemon's clock.
          const wait = Math.max(0, Date.parse(message.accepted) + MESSAGE_WAITING_DELAY - (Date.now() + ahead));
          later(`waiting:${event.id}`, wait, async () => show(messageNotice(await readMessage(event.id), opened + ahead, Date.now() + ahead)));
        });
      } else if (event.resource === "record") {
        if (wanted("schedule-skipped")) later("skips", READ_DELAY, readSkips);
        else {
          skipStart = Date.now();
          skipCursor = null;
        }
      }
    });
    return () => {
      unsubscribe();
      for (const timer of timers.values()) window.clearTimeout(timer);
    };
  }, [workspace]);
}
