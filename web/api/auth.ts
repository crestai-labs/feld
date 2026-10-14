// The browser's access to the daemon (design/auth.md#pairing). The browser token is an
// HttpOnly cookie the page cannot see and a key the page sends beside it, so the page learns
// it has none from a 401: every page then gives way to the pairing screen, which trades a
// code `feld ui pair` issued for the cookie and the key.

import { create } from "zustand";
import { storedValue, storeValue } from "../shell/storage.ts";

// The key lives in the storage of this page's origin, which a server on another port of the
// host never reaches though the cookie does.
const KEY_STORAGE = "feld.browserKey";

// The header of every request to the daemon that carries the key.
export function keyHeaders(): Record<string, string> {
  const key = storedValue(KEY_STORAGE, "");
  return key ? { "X-Feld-Browser-Key": key } : {};
}

// Why the pairing screen took the place of the page: a read answered 401 after the page
// had read (the token expired or was revoked), or the person removed this device.
export type PairingNotice = "ended" | "removed";
export type PairingResult = "paired" | "invalid" | "exhausted" | "cookieRejected";
// A sign-in without a result: the request never reached the daemon, the daemon answered
// with an error, or it took the code but the check of the cookie failed.
export interface PairingFailure {
  kind: "unreachable" | "refused" | "unconfirmed";
  message: string;
}

// An error the daemon answered, told apart from a request that never reached it.
export class DaemonRefusal extends Error {}

// The daemon drops every code after this many failed pairings in a row. It answers each
// failure alike, so the page counts its own.
export const PAIRING_FAILURE_LIMIT = 5;

interface AuthState {
  // A read the daemon answered: a later 401 means the access ended.
  answered: boolean;
  // The pairing screen is shown, with the reason when the page knows it.
  pairing: boolean;
  notice: PairingNotice | null;
}

export const useAuth = create<AuthState>(() => ({ answered: false, pairing: false, notice: null }));

export function noteAnswered() {
  if (!useAuth.getState().answered) useAuth.setState({ answered: true });
}

// The page gives way to the pairing screen; the first reason stays.
export function showPairing(notice: PairingNotice | null = null) {
  if (useAuth.getState().pairing) return;
  useAuth.setState({ pairing: true, notice });
}

// A request the daemon answered 401.
export function noteUnauthorized() {
  showPairing(useAuth.getState().answered ? "ended" : null);
}

// The daemon matches a code without regard to case and hyphens.
export function normalizeCode(code: string) {
  return code.toUpperCase().replace(/[\s-]/g, "");
}

// A light authenticated read used before spending a fragment's one-time code, and after
// pairing to make sure the browser kept the cookie and the key.
export async function browserAccess() {
  const response = await fetch("/v1/daemon/status", { cache: "no-store", headers: keyHeaders() });
  if (response.ok) return true;
  if (response.status === 401) return false;
  throw await refusalOf(response);
}

// `POST /v1/ui/pair`. The daemon answers with the cookie and the key; a code it does not
// hold is 401.
// A fixture passes its own `send` (`fixturePairingOf` in fixtures/index.ts).
export function pairingClient(send: (code: string) => Promise<boolean> = sendCode) {
  let failures = 0;
  return async (code: string): Promise<PairingResult> => {
    if (await send(code)) {
      failures = 0;
      return "paired";
    }
    failures += 1;
    if (failures < PAIRING_FAILURE_LIMIT) return "invalid";
    failures = 0;
    return "exhausted";
  };
}

// A sign-in with a code. `check` confirms the browser kept the cookie and the key; a fixture
// has none.
export async function signIn(pair: (code: string) => Promise<PairingResult>, code: string, check?: () => Promise<boolean>): Promise<PairingResult | PairingFailure> {
  let answer: PairingResult;
  try {
    answer = await pair(code);
  } catch (error) {
    return { kind: error instanceof DaemonRefusal ? "refused" : "unreachable", message: (error as Error).message };
  }
  if (answer !== "paired" || !check) return answer;
  try {
    return (await check()) ? "paired" : "cookieRejected";
  } catch (error) {
    return { kind: "unconfirmed", message: (error as Error).message };
  }
}

async function refusalOf(response: Response) {
  const body = await response.json().catch(() => ({}));
  return new DaemonRefusal(body.message || `${response.status} ${response.statusText}`);
}

async function sendCode(code: string) {
  const response = await fetch("/v1/ui/pair", {
    method: "POST",
    cache: "no-store",
    headers: { "Content-Type": "application/json", ...keyHeaders() },
    body: JSON.stringify({ code }),
  });
  if (response.ok) {
    storeValue(KEY_STORAGE, (await response.json()).key);
    return true;
  }
  if (response.status === 401) return false;
  throw await refusalOf(response);
}

// `POST /v1/ui/unpair`: the daemon revokes this browser's token and deletes its cookie, and
// the page forgets the key.
export async function unpair() {
  const response = await fetch("/v1/ui/unpair", { method: "POST", cache: "no-store", headers: keyHeaders() });
  if (!response.ok && response.status !== 401) {
    throw await refusalOf(response);
  }
  storeValue(KEY_STORAGE, "");
}
