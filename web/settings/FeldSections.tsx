import { useRef, useState, type ReactNode } from "react";
import { Link } from "react-router";
import * as prefs from "../shell/preferences.ts";
import { t } from "../shell/i18n.ts";
import { formatDuration, pluginRowsOf, skewSecondsOf, type PluginKind } from "../daemon/model.ts";
import type { DaemonAction, DaemonRead, DaemonSource } from "../daemon/source.ts";
import { Command, ConfirmDialog, CopyButton, instantText, Section } from "./parts.tsx";

// Feld's own sections of the Settings page: the daemon, its plugins, and the Workspaces,
// as the daemon answers them.

function Fact({ label, value, detail }: { label: string; value: ReactNode; detail?: string }) {
  return <div><dt>{label}</dt><dd><span>{value}</span>{detail ? <small>{detail}</small> : null}</dd></div>;
}

function Endpoint({ name, detail, url }: { name: string; detail: string; url: string | null }) {
  return (
    <li className={`feld-settings__endpoint${url ? "" : " is-down"}`}>
      <span className="feld-settings__dot" aria-hidden="true"></span>
      <span className="feld-settings__endpoint-name"><strong>{name}</strong><small>{detail}</small></span>
      {url ? <><code>{url}</code><CopyButton value={url} /></> : <span className="feld-settings__endpoint-problem">{t("daemon.notRunning")}</span>}
    </li>
  );
}

function Unreachable({ read, zone, onRetry }: { read: DaemonRead; zone: string; onRetry(): void }) {
  const { status, lastRead } = read;
  return (
    <div className="feld-settings__card feld-settings__unreachable">
      <h3>{t("daemon.unreachable.title")}</h3>
      {lastRead ? <p>{t("daemon.unreachable.detail", { at: instantText(Date.parse(lastRead), zone, { seconds: true }) })}</p> : null}
      <Command command="feld daemon start" />
      {status ? (
        <dl className="feld-settings__facts">
          <Fact label={t("daemon.version")} value={<code>{status.version}</code>} />
          <Fact label={t("daemon.lastPid")} value={<code>{status.pid}</code>} />
          <Fact label={t("daemon.api")} value={<code>{status.api}</code>} />
          <Fact label={t("daemon.home")} value={<code>{status.home}</code>} />
        </dl>
      ) : null}
      <button type="button" className="feld-settings__button" data-settings-action="retry" onClick={onRetry}>{t("daemon.retry")}</button>
    </div>
  );
}

// Only a reachable daemon is described: the process, what it keeps, and where it listens.
function Reachable({ read, zone, onConfirm }: { read: DaemonRead; zone: string; onConfirm(action: DaemonAction): void }) {
  const { now, restartedAt, actionError } = read;
  const status = read.status!;
  const started = Date.parse(status.startedAt);
  const skew = skewSecondsOf(status.serverTime, now);
  const { restored } = status;
  return (
    <>
      {status.otlp ? null : <p className="feld-settings__notice" role="status">{t("daemon.degraded")}</p>}
      <div className="feld-settings__card">
        <dl className="feld-settings__facts">
          <Fact label={t("daemon.version")} value={<code>{status.version}</code>} />
          <Fact label={t("daemon.process")} value={<>pid <code>{status.pid}</code></>} detail={t("daemon.instance", { id: status.instanceId })} />
          <Fact label={t("daemon.uptime")} value={formatDuration(now - started)} detail={t("daemon.started", { at: instantText(started, zone, { year: true }) })} />
          <Fact
            label={t("daemon.serverTime")} value={instantText(Date.parse(status.serverTime), zone, { seconds: true })}
            detail={skew === 0 ? t("daemon.skew.none") : t(skew > 0 ? "daemon.skew.ahead" : "daemon.skew.behind", { seconds: Math.abs(skew) })}
          />
          {status.historyStart
            ? <Fact label={t("daemon.history")} value={instantText(Date.parse(status.historyStart), zone, { year: true })} detail={t("daemon.historyKept", { duration: formatDuration(now - Date.parse(status.historyStart)) })} />
            : <Fact label={t("daemon.history")} value="—" />}
          <Fact label={t("daemon.home")} value={<code>{status.home}</code>} />
          <Fact label={t("daemon.timeZone")} value={<code>{status.timezone}</code>} detail={t("daemon.timeZone.detail")} />
          <Fact label={t("daemon.restored")} value={t("daemon.restored.value", { actors: restored.actors, sessions: restored.runningSessions, schedules: restored.schedules })} />
        </dl>
      </div>
      <div className="feld-settings__card">
        <h3 className="feld-settings__card-title">{t("daemon.endpoints")}</h3>
        <ul className="feld-settings__endpoints">
          <Endpoint name={t("daemon.api")} detail={t("daemon.api.detail")} url={status.api} />
          <Endpoint name={t("daemon.webUi")} detail={t("daemon.webUi.detail")} url={status.webUi} />
          <Endpoint name={t("daemon.collector")} detail={t("daemon.collector.detail")} url={status.otlp} />
        </ul>
      </div>
      <div className="feld-settings__card">
        <div className="feld-settings__row">
          <div className="feld-settings__row-copy">
            <span className="feld-settings__row-label">{t("daemon.control.label")}</span>
            <small>{t("daemon.control.detail")}</small>
          </div>
          <div className="feld-settings__row-control">
            <div className="feld-settings__buttons">
              <button type="button" className="feld-settings__button" data-settings-action="confirm-daemon" data-kind="restart" onClick={() => onConfirm("restart")}>{t("daemon.restart.button")}</button>
              <button type="button" className="feld-settings__button is-danger" data-settings-action="confirm-daemon" data-kind="stop" onClick={() => onConfirm("stop")}>{t("daemon.stop.button")}</button>
              {actionError
                ? <span className="feld-settings__status is-error" role="alert">{t(`daemon.${actionError.action}.failed`, { message: actionError.message })}</span>
                : restartedAt ? <span className="feld-settings__status" role="status">{t("daemon.restarted", { at: instantText(restartedAt, zone, { seconds: true }) })}</span> : null}
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

export function DaemonSection({ source }: { source: DaemonSource }) {
  const dialogs = { restart: useRef<HTMLDialogElement>(null), stop: useRef<HTMLDialogElement>(null) };
  // A read that lands while a confirmation is open must not take it away: the section shows
  // the read it had when the dialog opened until the dialog closes.
  const [held, setHeld] = useState<DaemonRead | null>(null);
  const read = held || source;
  const zone = prefs.timeZone(read.status?.timezone || null);

  function confirm(action: DaemonAction) {
    setHeld({ ...source });
    dialogs[action].current!.showModal();
  }

  function body() {
    if (read.phase === "reading") return null;
    if (read.phase === "unreachable") return <Unreachable read={read} zone={zone} onRetry={source.retry} />;
    return (
      <>
        <Reachable read={read} zone={zone} onConfirm={confirm} />
        {(["restart", "stop"] as const).map((action) => (
          <ConfirmDialog key={action} kind={`daemon.${action}`} danger={action === "stop"} dialogRef={dialogs[action]} onConfirm={() => void source[action]()} onClose={() => setHeld(null)} />
        ))}
      </>
    );
  }

  return <Section id="daemon" scope="feld">{body()}</Section>;
}

// A note under a section that shows the daemon's last answer while it is unreachable.
function StaleNote({ source }: { source: DaemonSource }) {
  if (source.phase !== "unreachable" || !source.lastRead) return null;
  const zone = prefs.timeZone(source.status?.timezone || null);
  return <p className="feld-settings__stale">{t("feld.stale", { at: instantText(Date.parse(source.lastRead), zone, { seconds: true }) })}</p>;
}

function PluginTable({ kind, installed }: { kind: PluginKind; installed: readonly FeldApi.Plugin[] }) {
  return (
    <div className="feld-settings__table-scroll">
      <table className="feld-settings__table">
        <thead><tr><th scope="col">{t("plugins.plugin")}</th><th scope="col">{t("plugins.status")}</th></tr></thead>
        <tbody>
          {pluginRowsOf(kind, installed).map((plugin) => (
            <tr key={plugin.name}>
              <th scope="row">
                <span className="feld-settings__plugin-name">
                  <strong>{plugin.label}</strong>
                  <code title={plugin.installed ? `$FELD_HOME/plugins/${kind}/${plugin.name}/` : undefined}>{plugin.name}</code>
                </span>
              </th>
              <td>
                <span className="feld-settings__plugin-status">
                  <span className={`feld-settings__source is-${plugin.installed ? "installed" : "missing"}`}>{t(plugin.installed ? "plugins.installed" : "plugins.notInstalled")}</span>
                  {plugin.installed ? null : <Command command={`feld init --${kind} ${plugin.name}`} />}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function PluginsSection({ source }: { source: DaemonSource }) {
  const { status } = source;
  return (
    <Section id="plugins" scope="feld">
      <StaleNote source={source} />
      {status ? (
        <>
          <div className="feld-settings__card">
            <h3 className="feld-settings__card-title">{t("plugins.runtimes")}<small>{t("plugins.runtimes.detail")}</small></h3>
            <PluginTable kind="runtime" installed={status.runtimes || []} />
          </div>
          <div className="feld-settings__card">
            <h3 className="feld-settings__card-title">{t("plugins.providers")}<small>{t("plugins.providers.detail")}</small></h3>
            <PluginTable kind="provider" installed={status.providers || []} />
          </div>
        </>
      ) : null}
    </Section>
  );
}

export function WorkspacesSection({ source }: { source: DaemonSource }) {
  const { workspaces, defaultWorkspace } = source;
  return (
    <Section id="workspaces" scope="feld">
      <StaleNote source={source} />
      {workspaces ? (
        <div className="feld-settings__card">
          <ul className="feld-settings__workspaces">
            {workspaces.map((key) => (
              <li key={key}>
                <Link className="feld-settings__workspace" to={`/ui/?workspace=${encodeURIComponent(key)}`}><code>{key}</code></Link>
                {key === defaultWorkspace ? <span className="feld-settings__source is-installed">{t("workspaces.default")}</span> : null}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </Section>
  );
}
