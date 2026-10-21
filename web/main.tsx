import { StrictMode, useEffect, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter, Navigate, Route, Routes, useLocation, useSearchParams } from "react-router";
import { showPairing, useAuth } from "./api/auth.ts";
import { createQueryClient, useDaemonTimeZone } from "./api/queries.ts";
import { useDaemonEvents } from "./api/events.ts";
import { Pairing } from "./shell/Pairing.tsx";
import { Shell } from "./shell/Shell.tsx";
import { useBrowserNotifications } from "./shell/notifications.ts";
import { ActorsPage } from "./actors/ActorsPage.tsx";
import { eventWorkspaceOf } from "./actors/url.ts";
import { DaemonSourceContext } from "./daemon/source.ts";
import { fixtureAuthStateOf, fixtureDaemonStateOf, fixturePairingOf, fixtureScenarioOf, useFixtureDaemonSource } from "./fixtures/index.ts";
import { SettingsPage } from "./settings/SettingsPage.tsx";
import "./shell/theme.ts";
import "./styles/shell.css";
import "./styles/console.css";
import "./styles/agents.css";
import "./styles/settings.css";
import "./styles/pairing.css";

// `?state=expired` of fixture mode: the page reads, then a later read answers 401.
// `?state=unauthorized` shows the pairing screen at once.
const FIXTURE_EXPIRY_DELAY = 4000;

// Each page names itself in the tab.
function Page({ title, children }: { title: string; children: ReactNode }) {
  useEffect(() => {
    document.title = title;
  }, [title]);
  return children;
}

// One event stream for the page, scoped to the Workspace it shows, and the browser
// notifications it raises. A fixture scenario reads no daemon.
function Events() {
  const { pathname } = useLocation();
  const [search] = useSearchParams();
  const fixture = fixtureScenarioOf(search) !== null || fixtureDaemonStateOf(search) !== null;
  const scope = fixture ? undefined : eventWorkspaceOf(pathname, search);
  useDaemonEvents(scope);
  useBrowserNotifications(scope, useDaemonTimeZone(!fixture));
  return null;
}

// The Settings page draws the fixture's daemon in fixture mode and reads the daemon otherwise.
function Settings() {
  const fixture = useFixtureDaemonSource();
  return <DaemonSourceContext.Provider value={fixture}><SettingsPage /></DaemonSourceContext.Provider>;
}

// The daemon is a section of the Settings page; its former page's links land there.
function DaemonRedirect() {
  const { search } = useLocation();
  return <Navigate to={`/ui/settings${search}#settings-daemon`} replace />;
}

// The pages, or the pairing screen in their place: always at `/ui/pair`, which `feld ui`
// opens, and on any page once the daemon answers 401 (api/auth.ts). Without the cookie
// the page's reads stop with the page.
function Pages() {
  const { pathname } = useLocation();
  const [search] = useSearchParams();
  const pairing = useAuth((state) => state.pairing);
  const notice = useAuth((state) => state.notice);
  const fixtureAuth = fixtureAuthStateOf(search);
  useEffect(() => {
    if (fixtureAuth !== "expired") return;
    const timer = window.setTimeout(() => showPairing("ended"), FIXTURE_EXPIRY_DELAY);
    return () => window.clearTimeout(timer);
  }, [fixtureAuth]);
  const pairPage = pathname === "/ui/pair";
  if (pairPage || pairing || fixtureAuth === "unauthorized") {
    return <Page title="Feld Sign In"><Pairing notice={pairPage ? null : notice} pairPage={pairPage} send={fixturePairingOf(search) ?? undefined} /></Page>;
  }
  return (
    <>
      <Events />
      <Shell>
        <Routes>
          <Route path="/ui/" element={<Page title="Feld Actor Activity"><ActorsPage /></Page>} />
          <Route path="/ui/daemon" element={<DaemonRedirect />} />
          <Route path="/ui/settings" element={<Page title="Feld Settings"><Settings /></Page>} />
          <Route path="*" element={<Navigate to="/ui/" replace />} />
        </Routes>
      </Shell>
    </>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={createQueryClient()}>
      <BrowserRouter>
        <Pages />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);
