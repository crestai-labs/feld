import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { browserAccess, DaemonRefusal, keyHeaders, noteAnswered, noteUnauthorized, normalizeCode, PAIRING_FAILURE_LIMIT, pairingClient, showPairing, signIn, unpair, useAuth } from "./auth.ts";

beforeEach(() => {
  useAuth.setState({ answered: false, pairing: false, notice: null });
});

afterEach(() => vi.unstubAllGlobals());

test("a 401 before any answer shows the pairing screen without a notice", () => {
  noteUnauthorized();
  expect(useAuth.getState()).toMatchObject({ pairing: true, notice: null });
});

test("a 401 after an answer says the access ended, and the first reason stays", () => {
  noteAnswered();
  noteUnauthorized();
  showPairing("removed");
  expect(useAuth.getState()).toMatchObject({ pairing: true, notice: "ended" });
});

test("a code is matched without regard to case, spaces, and hyphens", () => {
  expect(normalizeCode(" k7qm-4xpd ")).toBe("K7QM4XPD");
});

test("a run of failed codes is told apart from one, and a success counts again", async () => {
  const answers = [false, true, ...Array(PAIRING_FAILURE_LIMIT).fill(false)];
  const pair = pairingClient(async () => answers.shift()!);
  expect(await pair("A")).toBe("invalid");
  expect(await pair("B")).toBe("paired");
  const results = [];
  for (let attempt = 0; attempt < PAIRING_FAILURE_LIMIT; attempt += 1) results.push(await pair("C"));
  expect(results).toEqual([...Array(PAIRING_FAILURE_LIMIT - 1).fill("invalid"), "exhausted"]);
});

test("a light read tells whether the browser cookie works", async () => {
  const fetch = vi.fn()
    .mockResolvedValueOnce(new Response(null, { status: 200 }))
    .mockResolvedValueOnce(new Response(null, { status: 401 }));
  vi.stubGlobal("fetch", fetch);
  expect(await browserAccess()).toBe(true);
  expect(await browserAccess()).toBe(false);
  expect(fetch).toHaveBeenCalledWith("/v1/daemon/status", { cache: "no-store", headers: {} });
});

test("a pairing keeps the key the daemon answers, sends it beside the cookie, and signing out forgets it", async () => {
  const items = new Map<string, string>();
  vi.stubGlobal("localStorage", { getItem: (key: string) => items.get(key) ?? null, setItem: (key: string, value: string) => items.set(key, value) });
  const fetch = vi.fn()
    .mockResolvedValueOnce(new Response(JSON.stringify({ browser: { id: "browser_1" }, key: "k1" }), { status: 200 }))
    .mockResolvedValueOnce(new Response(JSON.stringify({ browser: { id: "browser_2" }, key: "k2" }), { status: 200 }))
    .mockResolvedValueOnce(new Response(null, { status: 204 }));
  vi.stubGlobal("fetch", fetch);
  expect(keyHeaders()).toEqual({});
  expect(await signIn(pairingClient(), "C")).toBe("paired");
  expect(keyHeaders()).toEqual({ "X-Feld-Browser-Key": "k1" });
  // Pairing again sends the key it replaces.
  expect(await signIn(pairingClient(), "D")).toBe("paired");
  expect(fetch.mock.calls[1][1].headers).toMatchObject({ "X-Feld-Browser-Key": "k1" });
  expect(keyHeaders()).toEqual({ "X-Feld-Browser-Key": "k2" });
  await unpair();
  expect(fetch.mock.calls[2][1].headers).toEqual({ "X-Feld-Browser-Key": "k2" });
  expect(keyHeaders()).toEqual({});
});

test("a failed cookie check keeps the daemon's reason", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(
    JSON.stringify({ message: "daemon unavailable" }),
    { status: 503, statusText: "Service Unavailable", headers: { "Content-Type": "application/json" } },
  )));
  const check = browserAccess();
  await expect(check).rejects.toThrow("daemon unavailable");
  await expect(check).rejects.toBeInstanceOf(DaemonRefusal);
});

test("a sign-in tells an absent daemon, a refusal, and a failed cookie check apart", async () => {
  const unreachable = async () => { throw new TypeError("Failed to fetch"); };
  const refused = async () => { throw new DaemonRefusal("the Origin of this request is not the address it is sent to"); };
  const paired = async () => "paired" as const;
  expect(await signIn(unreachable, "C")).toEqual({ kind: "unreachable", message: "Failed to fetch" });
  expect(await signIn(refused, "C", async () => true)).toEqual({ kind: "refused", message: "the Origin of this request is not the address it is sent to" });
  // The POST spent the code; only the check after it failed.
  expect(await signIn(paired, "C", unreachable)).toEqual({ kind: "unconfirmed", message: "Failed to fetch" });
  expect(await signIn(paired, "C", async () => false)).toBe("cookieRejected");
  expect(await signIn(paired, "C", async () => true)).toBe("paired");
  expect(await signIn(paired, "C")).toBe("paired");
});

test("the daemon's error answer to a pairing is a refusal", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(
    JSON.stringify({ message: "the Origin of this request is not the address it is sent to" }),
    { status: 403, headers: { "Content-Type": "application/json" } },
  )));
  expect(await signIn(pairingClient(), "C")).toEqual({ kind: "refused", message: "the Origin of this request is not the address it is sent to" });
});
