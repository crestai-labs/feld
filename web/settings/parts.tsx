import { useEffect, useState, type FormEvent, type ReactNode, type RefObject } from "react";
import * as prefs from "../shell/preferences.ts";
import { t } from "../shell/i18n.ts";

const COPIED_DURATION = 1400;

// An instant as the Actors page writes it, in the chosen zone and hour cycle.
export function instantText(at: number, zone: string, { seconds = false, year = false } = {}) {
  const text = new Intl.DateTimeFormat("en-US", {
    ...(year ? { year: "numeric" } : {}), month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
    ...(seconds ? { second: "2-digit" } : {}), hourCycle: prefs.hourCycle(), timeZone: zone,
  }).format(at);
  return `${text} ${prefs.zoneLabel(zone, at)}`;
}

// A section without a badge passes `badge={false}`: its facts are neither stored in this
// browser nor read from the daemon.
export function Section({ id, scope, badge = true, children }: { id: string; scope: "browser" | "feld"; badge?: boolean; children: ReactNode }) {
  return (
    <section className="feld-settings__group" id={`settings-${id}`} data-settings-section={id} aria-labelledby={`settings-${id}-title`}>
      <div className="feld-settings__section-heading">
        <div>
          <h2 id={`settings-${id}-title`}>{t(`${id}.title`)}</h2>
          <p>{t(`${id}.intro`)}</p>
        </div>
        {badge ? <span className={`feld-settings__scope is-${scope}`}>{t(scope === "feld" ? "scope.daemonBadge" : "scope.browserBadge")}</span> : null}
      </div>
      {children}
    </section>
  );
}

// One setting: what it is on the left, its control on the right.
export function Row({ id, label, detail, note, control }: { id?: string; label: string; detail: string; note?: string; control: ReactNode }) {
  return (
    <div className="feld-settings__row">
      <div className="feld-settings__row-copy">
        {id ? <label htmlFor={id}>{label}</label> : <span className="feld-settings__row-label">{label}</span>}
        <small>{detail}</small>
        {note ? <small className="feld-settings__row-note">{note}</small> : null}
      </div>
      <div className="feld-settings__row-control">{control}</div>
    </div>
  );
}

export function Segmented({ pref, label, options, checked }: { pref: prefs.PreferenceName; label: string; options: [string, string, string?][]; checked: string }) {
  return (
    <div className="feld-settings__segmented" role="radiogroup" aria-label={label}>
      {options.map(([value, text, hint]) => (
        <label key={value}>
          <input type="radio" name={`pref-${pref}`} value={value} data-pref={pref} checked={value === checked} onChange={(event) => event.target.checked && prefs.set(pref, value)} />
          <span>{text}{hint ? <> <small>{hint}</small></> : null}</span>
        </label>
      ))}
    </div>
  );
}

export function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), COPIED_DURATION);
    return () => window.clearTimeout(timer);
  }, [copied]);
  return (
    <button
      type="button"
      className={`feld-settings__copy${copied ? " is-copied" : ""}`}
      data-settings-action="copy"
      data-command={value}
      aria-label={`${t("feld.copy")}: ${value}`}
      title={t(copied ? "feld.copied" : "feld.copy")}
      onClick={() => {
        const done = () => setCopied(true);
        if (navigator.clipboard?.writeText) navigator.clipboard.writeText(value).then(done, done);
        else done();
      }}
    >
      <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="5.5" y="5.5" width="8" height="8" rx="1.5"></rect><path d="M10.5 3.5v-.5A1.5 1.5 0 0 0 9 1.5H3A1.5 1.5 0 0 0 1.5 3v6A1.5 1.5 0 0 0 3 10.5h.5"></path></svg>
    </button>
  );
}

export function Command({ command }: { command: string }) {
  return <span className="feld-settings__command"><code>{command}</code><CopyButton value={command} /></span>;
}

// A confirmation before the page resets all preferences, restarts or stops the daemon, or
// removes this device.
// `kind` names the phrases: <kind>.confirmTitle, <kind>.confirmDetail, and <kind>.label.
// The dialog's form closes the dialog by itself; the submitter says what was chosen.
export function ConfirmDialog({ kind, danger = false, dialogRef, onConfirm, onClose }: { kind: string; danger?: boolean; dialogRef: RefObject<HTMLDialogElement | null>; onConfirm(): void; onClose?(): void }) {
  const id = kind.replace(/\./g, "-");
  function submit(event: FormEvent<HTMLFormElement>) {
    if ((event.nativeEvent as SubmitEvent).submitter?.getAttribute("value") === "confirm") onConfirm();
  }
  return (
    <dialog className="feld-settings__dialog" ref={dialogRef} data-confirm-dialog={kind} aria-labelledby={`settings-${id}-title`} onClose={onClose}>
      <form method="dialog" onSubmit={submit}>
        <h3 id={`settings-${id}-title`}>{t(`${kind}.confirmTitle`)}</h3>
        <p>{t(`${kind}.confirmDetail`)}</p>
        <div className="feld-settings__dialog-actions">
          <button type="submit" className="feld-settings__button" value="cancel">{t("daemon.cancel")}</button>
          <button type="submit" className={`feld-settings__button${danger ? " is-danger" : " is-primary"}`} value="confirm">{t(`${kind}.label`)}</button>
        </div>
      </form>
    </dialog>
  );
}
