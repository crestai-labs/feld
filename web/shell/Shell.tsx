import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type PointerEvent, type ReactNode } from "react";
import { Link, NavLink, useLocation, useSearchParams } from "react-router";
import { DRAWER_ID, DrawerContext, type Drawer } from "./drawer.ts";
import { storedValue, storeValue } from "./storage.ts";
import { language, usePreferences } from "./preferences.ts";
import { t } from "./i18n.ts";
import { pageQuery } from "./workspace-url.ts";

const SIDEBAR_MIN_WIDTH = 176;
const SIDEBAR_MAX_WIDTH = 360;
const SIDEBAR_DEFAULT_WIDTH = 232;
// Mirrors the compact-shell media query in styles/shell.css.
const COMPACT_QUERY = "(max-width: 760px)";
const DRAWER_FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])';

function clampSidebarWidth(width: number) {
  const candidate = Number.isFinite(width) ? width : SIDEBAR_DEFAULT_WIDTH;
  return Math.round(Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, candidate)));
}

function useCompactShell() {
  const [compact, setCompact] = useState(() => window.matchMedia(COMPACT_QUERY).matches);
  useEffect(() => {
    const query = window.matchMedia(COMPACT_QUERY);
    const follow = () => setCompact(query.matches);
    query.addEventListener("change", follow);
    return () => query.removeEventListener("change", follow);
  }, []);
  return compact;
}

function NavItem({ to, title, children }: { to: string; title: string; children: ReactNode }) {
  return (
    <NavLink to={to} end title={title} className={({ isActive }) => (isActive ? "is-current" : undefined)}>
      {children}
      <span className="sidebar__label">{title}</span>
    </NavLink>
  );
}

// The sidebar and the page beside it. The wide layout starts with the sidebar collapsed to
// its icon rail so the Actors timeline and Console panes get the width; expanding it and
// its width are remembered. The compact layout shows the sidebar as a drawer opened from
// the top bar's menu button.
export function Shell({ children }: { children: ReactNode }) {
  const compact = useCompactShell();
  const [collapsed, setCollapsed] = useState(() => storedValue("feld-mock-sidebar", "collapsed") === "collapsed");
  const [width, setWidth] = useState(() => clampSidebarWidth(Number(storedValue("feld-mock-sidebar-width", String(SIDEBAR_DEFAULT_WIDTH)))));
  const [resizing, setResizing] = useState(false);
  const sidebar = useRef<HTMLElement>(null);
  // Moving between pages keeps the Workspace the Actors page shows and the fixture.
  const [search] = useSearchParams();
  const query = pageQuery(search);
  // The shell's words follow the language preference, and a reset returns the sidebar to
  // its stored (default) layout.
  usePreferences((state) => state.revision);
  const reset = usePreferences((state) => state.reset);
  useEffect(() => {
    document.documentElement.lang = language();
  });
  useEffect(() => {
    if (!reset) return;
    setCollapsed(storedValue("feld-mock-sidebar", "collapsed") === "collapsed");
    setWidth(clampSidebarWidth(Number(storedValue("feld-mock-sidebar-width", String(SIDEBAR_DEFAULT_WIDTH)))));
  }, [reset]);
  const resize = useRef<{ pointerId: number; startX: number; startWidth: number } | null>(null);
  // The compact shell's drawer has no collapsed form; the stored preference is kept for the
  // wide layout.
  const applied = collapsed && !compact;
  const menu = useRef<HTMLButtonElement>(null);
  const drawer = useShellDrawer(compact, sidebar, menu);

  useEffect(() => {
    document.body.classList.toggle("is-resizing-sidebar", resizing);
  }, [resizing]);

  function toggleCollapsed() {
    setCollapsed(!applied);
    storeValue("feld-mock-sidebar", applied ? "expanded" : "collapsed");
  }

  function commitWidth(next: number) {
    const clamped = clampSidebarWidth(next);
    setWidth(clamped);
    storeValue("feld-mock-sidebar-width", String(clamped));
  }

  function beginResize(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || applied) return;
    resize.current = { pointerId: event.pointerId, startX: event.clientX, startWidth: sidebar.current?.getBoundingClientRect().width || SIDEBAR_DEFAULT_WIDTH };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setResizing(true);
    event.preventDefault();
  }

  function moveResize(event: PointerEvent<HTMLDivElement>) {
    const gesture = resize.current;
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    setWidth(clampSidebarWidth(gesture.startWidth + event.clientX - gesture.startX));
  }

  function endResize(event: PointerEvent<HTMLDivElement>) {
    const gesture = resize.current;
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    resize.current = null;
    setResizing(false);
    commitWidth(sidebar.current?.getBoundingClientRect().width || gesture.startWidth);
  }

  function resizeByKey(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    commitWidth(width + (event.key === "ArrowLeft" ? -16 : 16));
  }

  return (
    <div className="viewport" data-sidebar-collapsed={applied ? "" : undefined} data-drawer-open={drawer.expanded ? "" : undefined} style={{ "--sidebar-width": `${width}px` } as React.CSSProperties}>
      <header className="topbar" inert={drawer.expanded}>
        <button ref={menu} className="topbar__menu" type="button" data-action="toggle-drawer" aria-label={t("nav.menu")} aria-expanded={drawer.expanded} aria-controls={DRAWER_ID} onClick={drawer.expanded ? drawer.close : drawer.open}>
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M3 5.5h14M3 10h14M3 14.5h14"></path></svg>
        </button>
      </header>

      <aside className="sidebar" id={DRAWER_ID} aria-label="Feld navigation" ref={sidebar} onClick={drawer.closeOnLink}>
        <header className="sidebar__brand">
          <Link className="sidebar__brand-lockup" to={`/ui/${query}`} aria-label="Feld home">
            <span className="sidebar__feld-logo" aria-hidden="true"></span>
            <strong className="sidebar__brand-name">Feld</strong>
          </Link>
        </header>

        <div className="sidebar__workspaces" ref={drawer.setSlot}></div>

        <nav className="sidebar__nav" aria-label="Primary navigation">
          <NavItem to={`/ui/${query}`} title={t("nav.actors")}>
            <svg className="sidebar__icon" viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="8" cy="8" r="3"></circle><circle cx="16" cy="16" r="3"></circle>
              <path d="M10.5 9.5 13.5 14.5M16 5v4m-2-2h4M8 15v4m-2-2h4"></path>
            </svg>
          </NavItem>
          <NavItem to={`/ui/settings${query}`} title={t("nav.settings")}>
            <svg className="sidebar__icon" viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="12" cy="12" r="3"></circle><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21h-4v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3v-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.5V3h4v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.5 1h.1v4h-.1a1.7 1.7 0 0 0-1.5 1Z"></path>
            </svg>
          </NavItem>
        </nav>

        <div className="sidebar__bottom">
          <button className="sidebar__collapse" type="button" data-action="toggle-sidebar" aria-label={t(applied ? "nav.expand" : "nav.collapse")} onClick={toggleCollapsed}>
            <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3" y="3" width="14" height="14" rx="2"></rect><path d="M8 3v14m5-10-3 3 3 3"></path></svg>
          </button>
        </div>
        <div
          className="sidebar__resize-handle"
          data-sidebar-resize=""
          role="separator"
          tabIndex={0}
          aria-label="Resize sidebar"
          aria-orientation="vertical"
          aria-valuemin={SIDEBAR_MIN_WIDTH}
          aria-valuemax={SIDEBAR_MAX_WIDTH}
          aria-valuenow={width}
          onPointerDown={beginResize}
          onPointerMove={moveResize}
          onPointerUp={endResize}
          onPointerCancel={endResize}
          onKeyDown={resizeByKey}
        ></div>
      </aside>

      {drawer.expanded ? <div className="topbar__scrim" aria-hidden="true" onClick={drawer.close}></div> : null}

      <main className="app" inert={drawer.expanded}>
        <DrawerContext value={drawer.context}>{children}</DrawerContext>
      </main>
    </div>
  );
}

// The compact layout's drawer. It opens with the focus on the current Workspace or page,
// and closes on the scrim, on Escape, and after a navigation; closing returns the focus to
// the control that opened it, or to the menu button after a page change.
function useShellDrawer(compact: boolean, sidebar: React.RefObject<HTMLElement | null>, menu: React.RefObject<HTMLElement | null>) {
  const [open, setOpen] = useState(false);
  const [slot, setSlot] = useState<HTMLDivElement | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const restoring = useRef(false);
  const { pathname, search } = useLocation();
  const expanded = compact && open;

  const context = useMemo<Drawer | null>(() => {
    if (!compact) return null;
    return {
      slot,
      expanded: open,
      open() {
        opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        setOpen(true);
      },
      close() {
        restoring.current = true;
        setOpen(false);
      },
    };
  }, [compact, slot, open]);

  useEffect(() => setOpen(false), [pathname, search, compact]);

  // The focus returns once the page is no longer inert.
  useEffect(() => {
    if (expanded || !restoring.current) return;
    restoring.current = false;
    opener.current?.focus({ preventScroll: true });
  }, [expanded]);

  useEffect(() => {
    if (!expanded) return undefined;
    const drawn = sidebar.current;
    const current = drawn?.querySelector<HTMLElement>('[aria-checked="true"]') ?? drawn?.querySelector<HTMLElement>(".is-current");
    current?.focus({ preventScroll: true });
    // Escape closes the drawer, and Tab cycles within it.
    function keepFocus(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") context?.close();
      if (event.key !== "Tab" || !drawn) return;
      const focusable = [...drawn.querySelectorAll<HTMLElement>(DRAWER_FOCUSABLE)].filter((item) => item.checkVisibility?.() ?? true);
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (!first || !last) return;
      if (event.shiftKey ? active === first || !drawn.contains(active) : active === last || !drawn.contains(active)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus({ preventScroll: true });
      }
    }
    document.addEventListener("keydown", keepFocus);
    return () => document.removeEventListener("keydown", keepFocus);
  }, [expanded, sidebar, context]);

  // A link in the drawer that leads to the page already shown closes it too. A link to
  // another page returns the focus to the menu button, since the page it leaves may take
  // the opener with it.
  function closeOnLink(event: MouseEvent<HTMLElement>) {
    const link = (event.target as Element).closest("a");
    if (!expanded || !link) return;
    if (link.pathname !== pathname || link.search !== search) opener.current = menu.current;
    context?.close();
  }

  return {
    context,
    expanded,
    setSlot,
    open: () => context?.open(),
    close: () => context?.close(),
    closeOnLink,
  };
}
