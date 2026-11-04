import { useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import * as prefs from "../shell/preferences.ts";
import { LANGUAGE_NAMES, t } from "../shell/i18n.ts";
import { useTheme } from "../shell/theme.ts";
import { DaemonSourceContext, useLiveDaemonSource, type DaemonSource } from "../daemon/source.ts";
import { badgeStateOf } from "../daemon/model.ts";
import { DaemonSection, PluginsSection, WorkspacesSection } from "./FeldSections.tsx";
import { ConfirmDialog, instantText, Row, Section, Segmented } from "./parts.tsx";

// The Settings page has two scopes, and each section says which it belongs to. The viewer's
// preferences (shell/preferences.ts) are stored in this browser and edited here. The daemon,
// its plugins, and the Workspaces are Feld's own: the page reads them from the daemon,
// restarts or stops the daemon, and names the CLI commands that change the rest.
const SECTIONS: [string, "browser" | "feld"][] = [
  ["general", "browser"], ["appearance", "browser"], ["console", "browser"], ["notifications", "browser"], ["device", "browser"],
  ["daemon", "feld"], ["plugins", "feld"], ["workspaces", "feld"],
];
const DARK_THEME_LABELS: Record<string, string> = { graphite: "Graphite", sumi: "Sumi", slate: "Slate" };
// How far below the top of the scroll a section counts as current: past the nav, which
// sticks over the sections in the narrow layout.
const CURRENT_SECTION_OFFSET = 72;

function DarkThemeSwatch({ name }: { name: string }) {
  return (
    <span className="feld-settings__swatch" data-dark-theme={name} aria-hidden="true">
      <span className="feld-settings__swatch-panel">
        <span className="feld-settings__swatch-ink"></span>
        <span className="feld-settings__swatch-soft"></span>
        <span className="feld-settings__swatch-accent"></span>
      </span>
      <span className="feld-settings__swatch-raised"></span>
    </span>
  );
}

function ThemeSwatch({ name }: { name: string }) {
  return (
    <span className={`feld-settings__theme-swatch is-${name}`} aria-hidden="true">
      <span className="feld-settings__theme-sidebar"></span>
      <span className="feld-settings__theme-content">
        <span></span><span></span><span></span>
      </span>
    </span>
  );
}

// The radio group's name also names its option attribute: data-<name>-option. "change"
// also covers arrow-key selection.
function Options({ options, name, selected, onSelect }: { options: [string, string, string, ReactNode][]; name: string; selected: string; onSelect(value: string): void }) {
  return (
    <div className="feld-settings__options">
      {options.map(([value, label, detail, swatch]) => (
        <label className="feld-settings__option" key={value}>
          <input type="radio" name={name} value={value} {...{ [`data-${name}-option`]: value }} checked={selected === value} onChange={(event) => event.target.checked && onSelect(value)} />
          {swatch}
          <span className="feld-settings__option-copy"><strong>{label}</strong><small>{detail}</small></span>
        </label>
      ))}
    </div>
  );
}

function allTimeZones(): readonly string[] {
  try {
    return Intl.supportedValuesOf("timeZone");
  } catch (_error) {
    return prefs.COMMON_ZONES;
  }
}

function TimeZoneSelect({ daemonZone, now }: { daemonZone: string | null; now: number }) {
  const choice = prefs.get("timeZone");
  const zoneOption = (zone: string) => `${zone} (${prefs.zoneLabel(zone, now)})`;
  const rest = allTimeZones().filter((zone) => !(prefs.COMMON_ZONES as readonly string[]).includes(zone));
  return (
    <select id="settings-time-zone" data-pref="timeZone" value={choice} onChange={(event) => prefs.set("timeZone", event.target.value)}>
      <option value="daemon">{t("timeZone.daemon", { zone: zoneOption(daemonZone || prefs.browserTimeZone()) })}</option>
      <option value="browser">{t("timeZone.browser", { zone: zoneOption(prefs.browserTimeZone()) })}</option>
      <optgroup label={t("timeZone.common")}>{prefs.COMMON_ZONES.map((zone) => <option key={zone} value={zone}>{zoneOption(zone)}</option>)}</optgroup>
      <optgroup label={t("timeZone.all")}>{rest.map((zone) => <option key={zone} value={zone}>{zone}</option>)}</optgroup>
    </select>
  );
}

function General({ daemonZone, now }: { daemonZone: string | null; now: number }) {
  const [resetStatus, setResetStatus] = useState("");
  const resetDialog = useRef<HTMLDialogElement>(null);
  const zone = prefs.timeZone(daemonZone);
  return (
    <Section id="general" scope="browser">
      <div className="feld-settings__card">
        <Row
          id="settings-language" label={t("language.label")} detail={t("language.detail")} note={t("language.scope")}
          control={(
            <select id="settings-language" data-pref="language" value={prefs.get("language")} onChange={(event) => prefs.set("language", event.target.value)}>
              <option value="system">{t("language.system", { language: LANGUAGE_NAMES[prefs.systemLanguage()] })}</option>
              {Object.entries(LANGUAGE_NAMES).map(([value, name]) => <option key={value} value={value} lang={value}>{name}</option>)}
            </select>
          )}
        />
        <Row id="settings-time-zone" label={t("timeZone.label")} detail={t("timeZone.detail")} control={<TimeZoneSelect daemonZone={daemonZone} now={now} />} />
        <Row
          label={t("hourCycle.label")} detail={t("hourCycle.detail")}
          control={<Segmented pref="hourCycle" label={t("hourCycle.label")} options={[["h23", t("hourCycle.h23"), "15:00"], ["h12", t("hourCycle.h12"), "3:00 PM"]]} checked={prefs.hourCycle()} />}
        />
        <Row label={t("preview.label")} detail={t("preview.detail")} control={<output className="feld-settings__preview" data-time-preview="">{instantText(now, zone)}</output>} />
        <Row
          label={t("reset.label")} detail={t("reset.detail")}
          control={(
            <div className="feld-settings__buttons">
              <button type="button" className="feld-settings__button" data-settings-action="reset-layout" onClick={() => { prefs.resetLayout(); setResetStatus(t("reset.layoutDone")); }}>{t("reset.layout")}</button>
              <button type="button" className="feld-settings__button is-danger" data-settings-action="reset-all" onClick={() => resetDialog.current!.showModal()}>{t("reset.all")}</button>
              <span className="feld-settings__status" data-reset-status="" role="status">{resetStatus}</span>
            </div>
          )}
        />
      </div>
      <ConfirmDialog kind="reset.all" danger dialogRef={resetDialog} onConfirm={() => { prefs.resetAll(); setResetStatus(t("reset.allDone")); }} />
    </Section>
  );
}

function Appearance() {
  const { theme, darkTheme, setTheme, setDarkTheme } = useTheme();
  const themes = (["system", "light", "dark"] as const).map((value): [string, string, string, ReactNode] => [value, t(`theme.${value}`), t(`theme.${value}.detail`), <ThemeSwatch name={value} />]);
  const darkThemes = Object.entries(DARK_THEME_LABELS).map(([value, label]): [string, string, string, ReactNode] => [value, label, t(`darkTheme.${value}.detail`), <DarkThemeSwatch name={value} />]);
  return (
    <Section id="appearance" scope="browser">
      <fieldset className="feld-settings__choices">
        <legend>{t("theme.legend")}</legend>
        <p className="feld-settings__choice-intro">{t("theme.intro")}</p>
        <Options options={themes} name="theme" selected={theme} onSelect={setTheme} />
      </fieldset>
      <fieldset className="feld-settings__choices feld-settings__dark-themes">
        <legend>{t("darkTheme.legend")}</legend>
        <p className="feld-settings__choice-intro">{t("darkTheme.intro")}</p>
        <Options options={darkThemes} name="dark-theme" selected={darkTheme} onSelect={setDarkTheme} />
      </fieldset>
    </Section>
  );
}

function ConsoleSection() {
  return (
    <Section id="console" scope="browser">
      <div className="feld-settings__card">
        <Row
          label={t("consoleView.label")} detail={t("consoleView.detail")}
          control={<Segmented pref="consoleView" label={t("consoleView.label")} options={[["conversation", "Conversation"], ["events", "All events"]]} checked={prefs.consoleView()} />}
        />
      </div>
    </Section>
  );
}

function permissionState() {
  return "Notification" in window ? Notification.permission : "unsupported";
}

function Notifications() {
  const [permission, setPermission] = useState(permissionState);
  const enabled = prefs.notifications();
  function toggle(name: string, checked: boolean) {
    const names = prefs.NOTIFICATIONS.filter((candidate) => (candidate === name ? checked : enabled.has(candidate)));
    prefs.set("notifications", names.join(","));
  }
  return (
    <Section id="notifications" scope="browser">
      <div className="feld-settings__card">
        <Row
          label={t("permission.label")} detail={t("permission.detail")}
          control={(
            <div className="feld-settings__buttons">
              <span className={`feld-settings__permission is-${permission}`}>{t(`permission.${permission}`)}</span>
              {permission === "default" ? (
                <button type="button" className="feld-settings__button" data-settings-action="request-notifications" onClick={() => void Promise.resolve(Notification.requestPermission()).finally(() => setPermission(permissionState()))}>
                  {t("permission.request")}
                </button>
              ) : null}
            </div>
          )}
        />
        <div className="feld-settings__checks" role="group" aria-labelledby="settings-notify-label">
          <span className="feld-settings__checks-label" id="settings-notify-label">{t("notify.label")}</span>
          <div className="feld-settings__check-grid">
            {prefs.NOTIFICATIONS.map((name) => (
              <label className="feld-settings__check" key={name}>
                <input type="checkbox" data-notification={name} checked={enabled.has(name)} onChange={(event) => toggle(name, event.target.checked)} />
                <span><strong>{t(`notify.${name}`)}</strong><small>{t(`notify.${name}.detail`)}</small></span>
              </label>
            ))}
          </div>
        </div>
      </div>
    </Section>
  );
}

// This browser's pairing with the daemon, which the person sees as Sign out. The browser
// holds its token as an HttpOnly cookie, so signing out asks the daemon, which needs a
// daemon that answers.
function Device({ source }: { source: DaemonSource }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const disabled = source.phase !== "reachable";
  function remove() {
    setFailure(null);
    source.removeDevice().catch((error: Error) => setFailure(error.message));
  }
  return (
    <Section id="device" scope="browser" badge={false}>
      <div className="feld-settings__card">
        <Row
          label={t("device.remove.label")} detail={t("device.remove.detail")}
          control={(
            <div className="feld-settings__buttons">
              <button type="button" className="feld-settings__button is-danger" data-settings-action="remove-device" disabled={disabled} onClick={() => dialog.current!.showModal()}>{t("device.remove.button")}</button>
              {failure ? <span className="feld-settings__status is-error" role="alert">{t("device.remove.failed", { message: failure })}</span> : null}
            </div>
          )}
        />
      </div>
      <ConfirmDialog kind="device.remove" danger dialogRef={dialog} onConfirm={remove} />
    </Section>
  );
}

function Nav({ current, onGo }: { current: string; onGo(id: string): void }) {
  const nav = useRef<HTMLElement>(null);
  // In the narrow layout the nav is one scrolling row; keep the marked link in it.
  useEffect(() => {
    const element = nav.current;
    const link = element?.querySelector<HTMLElement>(`[data-settings-link="${current}"]`);
    if (element && link && element.scrollWidth > element.clientWidth) element.scrollTo({ left: link.offsetLeft - 16 });
  }, [current]);
  const scopes = [["browser", "scope.browser"], ["feld", "scope.feld"]] as const;
  return (
    <nav className="feld-settings__nav" aria-label={t("settings.sections")} ref={nav}>
      {scopes.map(([scope, label]) => [
        <p className="feld-settings__nav-scope" key={scope}>{t(label)}</p>,
        ...SECTIONS.filter(([, sectionScope]) => sectionScope === scope).map(([id]) => (
          <a key={id} href={`#settings-${id}`} data-settings-link={id} aria-current={id === current ? "true" : undefined} onClick={(event) => { event.preventDefault(); onGo(id); }}>
            {t(`${id}.title`)}
          </a>
        )),
      ])}
      {/* The compact layout names the sections in a select in place of the row of links. */}
      <select className="feld-settings__nav-select" aria-label={t("settings.sections")} value={current} onChange={(event) => onGo(event.target.value)}>
        {scopes.map(([scope, label]) => (
          <optgroup key={scope} label={t(label)}>
            {SECTIONS.filter(([, sectionScope]) => sectionScope === scope).map(([id]) => <option key={id} value={id}>{t(`${id}.title`)}</option>)}
          </optgroup>
        ))}
      </select>
    </nav>
  );
}

function namedSection() {
  return location.hash.startsWith("#settings-") ? location.hash.slice("#settings-".length) : null;
}

export function SettingsView({ source }: { source: DaemonSource }) {
  // Every stored preference the page draws is read as it draws.
  const revision = prefs.usePreferences((state) => state.revision);
  const body = useRef<HTMLDivElement>(null);
  const named = namedSection();
  const [current, setCurrent] = useState(named || SECTIONS[0][0]);
  const daemonZone = source.status?.timezone || null;

  // A link names its section; the scroll marks the section at its top.
  function markCurrentSection(section: string | null = null) {
    const element = body.current;
    if (section) setCurrent(section);
    if (section || !element) return;
    const top = element.getBoundingClientRect().top + CURRENT_SECTION_OFFSET;
    let found = SECTIONS[0][0];
    for (const section of element.querySelectorAll<HTMLElement>("[data-settings-section]")) {
      if (section.getBoundingClientRect().top <= top) found = section.dataset.settingsSection!;
    }
    if (element.scrollTop + element.clientHeight >= element.scrollHeight - 2) found = SECTIONS[SECTIONS.length - 1][0];
    setCurrent(found);
  }

  // An explicit smooth scroll ignores the stylesheet, so reduced motion is checked here.
  function go(id: string, behavior: ScrollBehavior = matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth") {
    body.current?.querySelector(`#settings-${id}`)?.scrollIntoView({ block: "start", behavior });
    history.replaceState(history.state, "", `${location.pathname}${location.search}#settings-${id}`);
    setCurrent(id);
  }

  // A page opened on a section's link (`/ui/daemon` is one) starts there, once the daemon's
  // sections below have their height.
  const settled = source.phase !== "reading";
  const opened = useRef(false);
  useLayoutEffect(() => {
    if (!settled || opened.current) return;
    opened.current = true;
    if (named) go(named, "auto");
    // Only the link the page opened on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settled]);

  // As the sections change and as the hash changes, the nav marks again: the section the
  // hash names, or else the one the scroll shows.
  useLayoutEffect(() => {
    markCurrentSection(namedSection());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source.phase, source.workspaces === null, revision]);
  useEffect(() => {
    const onHashChange = () => markCurrentSection(namedSection());
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const badge = source.phase === "reading" ? null : badgeStateOf(source.phase, source.status);
  return (
    <feld-settings>
      <section className="feld-settings" aria-label={t("settings.title")}>
        <header className="feld-settings__heading">
          <div className="feld-settings__heading-copy">
            <h1>{t("settings.title")}</h1>
          </div>
          {badge ? (
            <a className={`feld-settings__daemon-badge is-${badge}`} href="#settings-daemon" data-settings-link="daemon" onClick={(event) => { event.preventDefault(); go("daemon"); }}>
              {t("daemon.badge", { state: t(`daemon.state.${badge}`) })}
            </a>
          ) : null}
        </header>
        <div className="feld-settings__body" ref={body} onScroll={() => markCurrentSection()}>
          <div className="feld-settings__layout">
            <Nav current={current} onGo={go} />
            <div className="feld-settings__sections">
              <General daemonZone={daemonZone} now={source.now} />
              <Appearance />
              <ConsoleSection />
              <Notifications />
              <Device source={source} />
              <DaemonSection source={source} />
              <PluginsSection source={source} />
              <WorkspacesSection source={source} />
            </div>
          </div>
        </div>
      </section>
    </feld-settings>
  );
}

function LiveSettingsPage() {
  return <SettingsView source={useLiveDaemonSource()} />;
}

// A fixture adapter provides the daemon through DaemonSourceContext; without one the page
// reads the daemon.
export function SettingsPage() {
  const fixture = useContext(DaemonSourceContext);
  return fixture ? <SettingsView source={fixture} /> : <LiveSettingsPage />;
}
