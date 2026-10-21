// The page heading: the Workspace switcher, the reads' connection, Refresh and the Live /
// Paused toggle, the search, and New actor. The range button that opens the range
// picker sits in the Actors section's header. On the compact layout the heading is the top
// bar beside the shell's menu button: the search opens from its button, and the Workspace
// list sits in the shell's drawer.

import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type FocusEvent as ReactFocusEvent, type KeyboardEvent as ReactKeyboardEvent, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useShallow } from "zustand/react/shallow";
import { DRAWER_ID, useDrawer } from "../shell/drawer.ts";
import { useActorsModel } from "./context.ts";
import { COMPACT_LAYOUT_QUERY, METRICS_WINDOWS, MODE_TITLES, WINDOW_NAMES, WINDOW_OPTIONS, metricsDurationFor } from "./model.ts";
import { useActorsStore } from "./store.ts";
import { focusIn, useActorsTools, usePageActions } from "./ui.tsx";

const CHECK = <svg className="sidebar__check" viewBox="0 0 20 20" aria-hidden="true"><path d="m4 10 4 4 8-9"></path></svg>;

export function Heading() {
  const actions = usePageActions();
  const query = useActorsStore((state) => state.query);
  const newActorOpen = useActorsStore((state) => Boolean(state.newActor));
  const shown = useShownConnection();
  const { afterRender } = useActorsTools();
  // The compact layout shows the search field while it is opened or holds a query.
  const [searchOpen, setSearchOpen] = useState(false);
  const searching = searchOpen || query !== "";
  function toggleSearch() {
    if (searching && !query) {
      setSearchOpen(false);
      return;
    }
    setSearchOpen(true);
    afterRender((root) => focusIn(root, '[data-action="search"]'));
  }
  return (
    <div className={`feld-agents__heading${searching ? " is-searching" : ""}`}>
      <div className="feld-agents__heading-copy">
        <WorkspaceSwitcher />
      </div>
      <div className="feld-agents__heading-controls">
        <Connection shown={shown} />
        {/* The button keeps the focus in the field it closes (onMouseDown), so a blur never reopens it. */}
        <button className="feld-agents__button feld-agents__button--search" type="button" data-action="toggle-search" aria-label="Search actors and metrics" aria-expanded={searching} aria-controls="feld-agents-search" onMouseDown={(event) => event.preventDefault()} onClick={toggleSearch}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.6"></circle><path d="m10.4 10.4 3.4 3.4"></path></svg>
        </button>
        <RefreshControls shown={shown} />
        <input className="feld-agents__search" id="feld-agents-search" type="search" placeholder="Search actors and metrics" aria-label="Search actors and metrics" title="Matches Actor names, Coding Agents, Execution Providers, metric names, Runtimes, and accounts" value={query} data-action="search" onChange={(event) => useActorsStore.getState().setQuery(event.target.value)} onBlur={() => setSearchOpen(false)} />
        <button className="feld-agents__button feld-agents__button--primary feld-agents__button--new" type="button" data-action="new-actor" aria-haspopup="dialog" aria-expanded={newActorOpen} title="Declare an Agent or a Job" onClick={actions.openNewActor}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 3v10M3 8h10"></path></svg>
          <span>New actor</span>
        </button>
      </div>
    </div>
  );
}

// How long a wait lasts before the connection says so, so a read that answers at once
// does not flash it.
const CONNECTION_DELAY_MS = 250;

// The reads behind the page. Nothing is shown while they succeed, and a wait only once it
// has lasted CONNECTION_DELAY_MS. A failure shows at once and puts the failed read's
// message in the tooltip (Refresh's too), because the page then shows an older picture.
// The words fill only the room the controls leave free (see .feld-agents__connection).
function Connection({ shown }: { shown: FeldLiveConnection | null }) {
  return <span className={`feld-agents__connection${shown ? ` is-${shown.state}` : ""}`} role="status" title={shown ? shown.detail || shown.message : undefined}>{shown?.message || ""}</span>;
}

function useShownConnection(): FeldLiveConnection | null {
  const { connection } = useActorsTools();
  const state = connection?.state ?? "live";
  const waiting = state === "connecting" || state === "reconnecting";
  const [waited, setWaited] = useState<string | null>(null);
  useEffect(() => {
    if (!waiting) return undefined;
    const timer = window.setTimeout(() => setWaited(state), CONNECTION_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
      setWaited(null);
    };
  }, [state, waiting]);
  if (!connection || state === "live") return null;
  return !waiting || waited === state ? connection : null;
}

const REFRESH_GLYPH = <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13.2 8a5.2 5.2 0 1 1-1.6-3.8"></path><path d="M13.4 2.6v3.2h-3.2"></path></svg>;

// Refresh and the Live / Paused toggle, one joined group, in the page heading since they
// re-read both the Actors and the Metrics. Refresh reads every range read again and turns
// its icon once. Paused holds a Live range until Refresh or Live again.
function RefreshControls({ shown }: { shown: FeldLiveConnection | null }) {
  const { client } = useActorsTools();
  const live = useActorsStore((state) => state.mode === "live");
  const paused = useActorsStore((state) => state.paused);
  const button = useRef<HTMLButtonElement>(null);
  function refresh() {
    useActorsStore.getState().refresh();
    client?.invalidateQueries();
    const element = button.current;
    if (!element) return;
    element.classList.remove("is-refreshing");
    void element.offsetWidth;
    element.classList.add("is-refreshing");
  }
  return (
    <div className={`feld-agents__timebar feld-agents__refresh${shown ? ` is-${shown.state}` : ""}`} role="group" aria-label="Refresh">
      <button className="feld-agents__button feld-agents__button--refresh" type="button" data-action="refresh" aria-label="Refresh" title={`${shown?.state === "error" ? `Read failed: ${shown.detail} · ` : ""}Refresh: re-read the range now`} ref={button} onClick={refresh}>{REFRESH_GLYPH}</button>
      <button className={`feld-agents__button feld-agents__button--live${paused ? " is-paused" : ""}`} type="button" data-action="toggle-paused" title={live ? (paused ? "Paused: the range holds until Refresh; click to follow it live" : "Live: the range re-reads on each change; click to pause it") : "A historical range does not change; pausing applies to a Live range"} disabled={!live} onClick={() => useActorsStore.getState().setPaused(!paused)}>{paused ? "Paused" : "Live"}</button>
    </div>
  );
}

const RANGE_CLOCK_GLYPH = <svg className="feld-agents__range-clock" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.2"></circle><path d="M8 4.6V8l2.4 1.6"></path></svg>;

// The range of the Timeline (and, through it, of Metrics): opens the picker.
export function TimeControls() {
  const model = useActorsModel();
  const range = useActorsStore(useShallow((state) => ({ mode: state.mode, anchor: state.anchor, duration: state.duration })));
  const pickerOpen = useActorsStore((state) => state.pickerOpen);
  return (
    <div className="feld-agents__timebar" role="group" aria-label="Time range">
      <button className="feld-agents__button feld-agents__button--range" type="button" data-action="toggle-picker" aria-expanded={pickerOpen} aria-haspopup="dialog" data-range-button="" title={MODE_TITLES[range.mode]} onClick={() => useActorsStore.getState().togglePicker()}>
        {RANGE_CLOCK_GLYPH}
        <span data-range-label="">{model.rangeButtonLabel(range)}</span>
        <span className="feld-agents__range-caret" aria-hidden="true">▾</span>
      </button>
    </div>
  );
}

// The Workspace switcher: one Workspace (the range reads with a workspace query) or all of
// them (no query). Actors of other Workspaces are named workspace/actor; with all
// Workspaces shown every Actor is. A menu item, a click outside the switcher, and Escape
// close the menu, and so does the focus leaving the switcher, so the page's Escape never
// reaches past an open menu. Escape closes only the menu, and returns the focus to its button.
// On the compact layout the button opens the shell's drawer, which holds the menu's items.
function WorkspaceSwitcher() {
  const model = useActorsModel();
  const actions = usePageActions();
  const drawer = useDrawer();
  const inDrawer = drawer?.slot ?? null;
  const workspace = useActorsStore((state) => state.workspace);
  const [open, setOpen] = useState(false);
  const switcher = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return undefined;
    const closeOutside = (event: MouseEvent) => {
      if (!switcher.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("click", closeOutside);
    return () => document.removeEventListener("click", closeOutside);
  }, [open]);

  // The button takes the focus itself, since a click does not focus a button everywhere
  // (Safari), so the menu's Escape and focus-out start inside the switcher.
  function toggle() {
    if (drawer) {
      drawer.open();
      return;
    }
    setOpen(!open);
    button.current?.focus();
  }

  function closeOnEscape(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (!open || event.key !== "Escape") return;
    event.stopPropagation();
    setOpen(false);
    button.current?.focus();
  }

  // A blur with nowhere to go (a click on a non-focusable spot) is left to closeOutside.
  function closeOnFocusOut(event: ReactFocusEvent<HTMLDivElement>) {
    const next = event.relatedTarget;
    if (next && !event.currentTarget.contains(next)) setOpen(false);
  }

  function select(next: string | null) {
    setOpen(false);
    drawer?.close();
    useActorsStore.getState().selectWorkspace(next);
  }

  return (
    <div className="feld-agents__workspace-switcher" ref={switcher} onKeyDown={closeOnEscape} onBlur={closeOnFocusOut}>
      <h1>
        <button className="feld-agents__workspace-button" type="button" data-action="toggle-workspace-menu" aria-expanded={drawer ? drawer.expanded : open} aria-haspopup={drawer ? "true" : "menu"} aria-controls={drawer ? DRAWER_ID : undefined} aria-label="Select workspace" ref={button} onClick={toggle}>
          <span>{workspace ? workspace : "All workspaces"}</span>
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m6 8 4 4 4-4"></path></svg>
        </button>
      </h1>
      {portalInto(inDrawer, <div className="sidebar__popover feld-agents__workspace-menu" data-workspace-menu="" role="menu" aria-label="Workspaces" hidden={inDrawer ? false : !open}>
        {model.input.workspaces.map((key) => (
          <button key={key} type="button" data-action="select-workspace" data-workspace={key} role="menuitem" aria-checked={workspace === key} onClick={() => select(key)}>
            <svg className="sidebar__icon" viewBox="0 0 24 24" aria-hidden="true">
              <rect x="5" y="3" width="14" height="18" rx="2"></rect>
              <path d="M9 7h2m2 0h2M9 11h2m2 0h2M9 15h2m2 0h2M10 21v-3h4v3"></path>
            </svg>
            <span>{key}</span>
            {workspace === key ? CHECK : null}
          </button>
        ))}
        <button type="button" data-action="all-workspaces" role="menuitem" aria-checked={workspace === null} onClick={() => select(null)}><span>All workspaces</span>{workspace === null ? CHECK : null}</button>
        <button type="button" data-action="new-workspace" role="menuitem" aria-haspopup="dialog" onClick={() => { setOpen(false); drawer?.close(); actions.openNewWorkspace(); }}>
          <svg className="sidebar__icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14"></path></svg>
          <span>New workspace</span>
        </button>
      </div>)}
    </div>
  );
}

function portalInto(target: HTMLElement | null, children: ReactNode) {
  return target ? createPortal(children, target) : children;
}

// The Metrics window: opens its picker. Live it names the window's length, else its dates.
export function MetricsWindowControl() {
  const model = useActorsModel();
  const state = useActorsStore(useShallow((current) => ({ mode: current.mode, anchor: current.anchor, duration: current.duration, metricsDuration: current.metricsDuration })));
  const open = useActorsStore((current) => current.metricsPickerOpen);
  return (
    <div className="feld-agents__timebar" role="group" aria-label="Metrics window">
      <button className="feld-agents__button feld-agents__button--range" type="button" data-action="toggle-metrics-picker" aria-expanded={open} aria-haspopup="dialog" data-metrics-button="" title="The Metrics window: never narrower than the Timeline's, and centred on it" onClick={() => useActorsStore.getState().toggleMetricsPicker()}>
        {RANGE_CLOCK_GLYPH}
        <span data-metrics-range="">{model.metricsButtonLabel(state)}</span>
        <span className="feld-agents__range-caret" aria-hidden="true">▾</span>
      </button>
    </div>
  );
}

// A picker opens under the button that opened it, or above it when it would run past the
// component's bottom and fits there (the Metrics window sits low on the page), its left
// edge aligned with the button's and kept inside the component. On the compact layout it
// spans the viewport and scrolls inside whatever height remains below it.
function usePickerPlacement(picker: RefObject<HTMLDivElement | null>, buttonSelector: string) {
  const [placement, setPlacement] = useState<CSSProperties>({});
  useLayoutEffect(() => {
    const element = picker.current;
    const section = element?.closest<HTMLElement>(".feld-agents");
    const button = section?.querySelector(buttonSelector);
    if (!element || !section || !button) return;
    const sectionRect = section.getBoundingClientRect();
    const buttonRect = button.getBoundingClientRect();
    const below = buttonRect.bottom - sectionRect.top + 6;
    const above = buttonRect.top - sectionRect.top - 6 - element.offsetHeight;
    const openUp = below + element.offsetHeight > section.clientHeight - 8 && above >= 8;
    let next: CSSProperties;
    if (!window.matchMedia(COMPACT_LAYOUT_QUERY).matches) {
      const maxLeft = section.clientWidth - element.offsetWidth - 8;
      next = { top: `${openUp ? above : below}px`, left: `${Math.max(8, Math.min(buttonRect.left - sectionRect.left, maxLeft))}px` };
    } else {
      next = { top: `${openUp ? above : below}px`, maxHeight: openUp ? undefined : `${Math.max(160, section.clientHeight - below - 8)}px` };
    }
    setPlacement((previous) => (previous.top === next.top && previous.left === next.left && previous.maxHeight === next.maxHeight ? previous : next));
  });
  return placement;
}

// A picker closes when the pointer goes down anywhere outside it, apart from the button
// that opens it, whose own press toggles it. The press closes it before the click lands,
// so a control pressed elsewhere acts with the picker already gone.
function useCloseOnOutsidePress(picker: RefObject<HTMLDivElement | null>, buttonSelector: string, close: () => void) {
  useEffect(() => {
    function onPress(event: PointerEvent) {
      const target = event.target as Element | null;
      if (!target || picker.current?.contains(target) || target.closest(buttonSelector)) return;
      close();
    }
    document.addEventListener("pointerdown", onPress, true);
    return () => document.removeEventListener("pointerdown", onPress, true);
  }, [picker, buttonSelector, close]);
}

// Quick ranges for Metrics only: the window's length, placed by the Timeline. A length
// narrower than the Timeline's cannot be chosen.
export function MetricsPicker() {
  const open = useActorsStore((state) => state.metricsPickerOpen);
  return open ? <MetricsPickerDialog /> : null;
}

function MetricsPickerDialog() {
  const store = useActorsStore.getState;
  const duration = useActorsStore((state) => state.duration);
  const chosen = useActorsStore((state) => metricsDurationFor(state));
  const picker = useRef<HTMLDivElement>(null);
  const placement = usePickerPlacement(picker, "[data-metrics-button]");
  useCloseOnOutsidePress(picker, "[data-metrics-button]", store().closeMetricsPicker);
  return (
    <div className="feld-agents__picker feld-agents__metrics-picker" role="dialog" aria-label="Select the Metrics window" ref={picker} style={placement}>
      <div>
        <h2>Quick ranges</h2>
        <div className="feld-agents__quick-ranges">
          {METRICS_WINDOWS.map((value) => {
            const narrower = value < duration;
            return (
              <button key={value} type="button" data-action="metrics-quick" data-duration={value} aria-current={value === chosen ? "true" : undefined} disabled={narrower} title={narrower ? `Narrower than the Timeline's ${WINDOW_NAMES[duration] || ""}; Metrics is never narrower` : undefined} onClick={() => store().setMetricsDuration(value)}>Last {WINDOW_NAMES[value]}</button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// The range picker opens under the range button. The historical fields start at the range
// shown and keep what is typed until Apply.
export function Picker() {
  const open = useActorsStore((state) => state.pickerOpen);
  return open ? <PickerDialog /> : null;
}

function PickerDialog() {
  const model = useActorsModel();
  const store = useActorsStore.getState;
  const picker = useRef<HTMLDivElement>(null);
  const [date, setDate] = useState(() => model.toDateInput(store().anchor));
  const [time, setTime] = useState(() => model.timeInput.format(store().anchor));
  const [duration, setDuration] = useState(() => store().duration);
  const placement = usePickerPlacement(picker, "[data-range-button]");
  useCloseOnOutsidePress(picker, "[data-range-button]", store().closePicker);

  return (
    <div className="feld-agents__picker" role="dialog" aria-label="Select time range" ref={picker} style={placement}>
      <div>
        <h2>Quick ranges</h2>
        <div className="feld-agents__quick-ranges">
          {WINDOW_OPTIONS.map(([label, value]) => (
            <button key={value} type="button" data-action="quick-live" data-duration={value} aria-current={store().mode === "live" && value === store().duration ? "true" : undefined} onClick={() => store().quickLive(value, model.now())}>Last {WINDOW_NAMES[value] || label}</button>
          ))}
        </div>
      </div>
      <div>
        <h2>Historical range</h2>
        <div className="feld-agents__picker-fields">
          <label className="feld-agents__picker-feld">
            Anchor date
            <input type="date" value={date} data-picker="date" onChange={(event) => setDate(event.target.value)} />
          </label>
          <label className="feld-agents__picker-feld">
            Anchor time
            <input type="time" value={time} data-picker="time" onChange={(event) => setTime(event.target.value)} />
          </label>
          <label className="feld-agents__picker-feld">
            Window
            <select data-picker="duration" value={duration} onChange={(event) => setDuration(Number(event.target.value))}>
              {WINDOW_OPTIONS.map(([label, value]) => <option key={value} value={value}>{label}</option>)}
            </select>
          </label>
          <label className="feld-agents__picker-feld">
            Timezone
            <select disabled><option>{model.TIME_ZONE}</option></select>
          </label>
        </div>
        <p className="feld-agents__picker-note">
          The anchor stays fixed when the window changes. A 1 day window means 24 hours, not a calendar day.
        </p>
        <div className="feld-agents__picker-actions">
          <button className="feld-agents__button" type="button" data-action="close-picker" onClick={() => store().closePicker()}>Cancel</button>
          <button className="feld-agents__button" type="button" data-action="apply-picker" onClick={() => store().applyPicker(model.instantOfLocal(date, time), duration, model.now())}>Apply range</button>
        </div>
      </div>
    </div>
  );
}
