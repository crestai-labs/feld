// What the Settings page makes of its daemon reads: the phase, the badge, the plugin rows,
// and the instants as the page shows them. Everything here is pure; the page and its tests
// share it.

// `phase` is what the last read made of the daemon: reading, reachable, or unreachable.
// A read the daemon answers 401 gives the page over to the pairing screen (api/auth.ts).
export type Phase = "reading" | "reachable" | "unreachable";
export type BadgeState = "running" | "degraded" | "unreachable";
export type PluginKind = "runtime" | "provider";

export interface PluginRow {
  name: string;
  label: string;
  installed: boolean;
}

// While the daemon answers, the page follows its notifications and reads again on this
// interval so an unchanged daemon still shows a fresh instant.
export const POLL_INTERVAL = 30000;
// While the daemon is silent, the page tries again on this one.
export const RETRY_INTERVAL = 5000;
// While a restart is under way, the page reads on this one, and gives the new process this
// long to answer before it takes the daemon as unreachable.
export const RESTART_READ_INTERVAL = 1000;
export const RESTART_LIMIT = 30000;

// The plugins Feld ships (design/plugin.md). The table lists each one, installed or not,
// so what the home lacks is as visible as what it has.
export const BUNDLED_PLUGINS: readonly [PluginKind, string, string][] = [
  ["runtime", "claude-code", "Claude Code"], ["runtime", "codex", "Codex"], ["provider", "tmux", "tmux"], ["provider", "herdr", "Herdr"],
];

// The next read after the last one, or none before the first read answers.
export function readIntervalOf(phase: Phase): number | false {
  if (phase === "reading") return false;
  return phase === "reachable" ? POLL_INTERVAL : RETRY_INTERVAL;
}

// The outcome of the daemon read as TanStack Query keeps it: the newest of a success, a
// failure, and the moment the page learned the daemon is on its way out (`goneAt`, the
// shutdown notification or an accepted stop or restart) decides the phase.
export interface ReadOutcome {
  status: "pending" | "error" | "success";
  error: unknown;
  dataUpdatedAt: number;
  errorUpdatedAt: number;
}

export function phaseOf(outcome: ReadOutcome, goneAt: number): Phase {
  const latest = Math.max(outcome.dataUpdatedAt, outcome.errorUpdatedAt);
  if (goneAt > latest) return "unreachable";
  if (outcome.status === "pending") return "reading";
  if (outcome.status === "error") return "unreachable";
  return "reachable";
}

// A restart the daemon accepted: the instant, and the instance it asked to restart.
export interface Restart {
  at: number;
  instanceId: string;
}

// A restart is under way until a read names another instance, within the limit. Meanwhile
// the page shows the daemon it last read, running again from the restart.
export function restartingOf(restart: Restart | null, status: FeldApi.DaemonStatus | null, now: number): boolean {
  return restart !== null && status?.instanceId === restart.instanceId && now - restart.at < RESTART_LIMIT;
}

export function badgeStateOf(phase: Phase, status: FeldApi.DaemonStatus | null): BadgeState {
  if (phase === "reachable") return status?.otlp ? "running" : "degraded";
  return "unreachable";
}

// Every bundled plugin of the kind, installed or not, then any other plugin the home has.
export function pluginRowsOf(kind: PluginKind, installed: readonly FeldApi.Plugin[]): PluginRow[] {
  const bundled = BUNDLED_PLUGINS.filter(([pluginKind]) => pluginKind === kind);
  const rows = bundled.map(([, name, label]) => {
    const plugin = installed.find((entry) => entry.name === name);
    return { name, label: plugin?.label || label, installed: Boolean(plugin) };
  });
  const others = installed.filter((plugin) => !bundled.some(([, name]) => name === plugin.name))
    .map((plugin) => ({ name: plugin.name, label: plugin.label, installed: true }));
  return [...rows, ...others];
}

export function formatDuration(ms: number) {
  const minutes = Math.floor(Math.abs(ms) / 60000);
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  const rest = minutes % 60;
  if (days > 0) return `${days}d ${hours}h ${rest}m`;
  if (hours > 0) return `${hours}h ${rest}m`;
  return `${rest}m`;
}

// How far the daemon's clock is from this browser's, in whole seconds (never -0).
export function skewSecondsOf(serverTime: string, now: number) {
  return Math.round((Date.parse(serverTime) - now) / 1000) + 0;
}
