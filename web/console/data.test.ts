import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { QueryClient, type InfiniteData } from "@tanstack/react-query";
import type { ConsolePage } from "./model.ts";

// The daemon as the Console reads it: a transcript that grows, and every request made.
const requests: string[] = [];
let transcript: FeldApi.TranscriptEntry[] = [];

function entry(index: number): FeldApi.TranscriptEntry {
  return { cursor: String(index).padStart(3, "0"), at: new Date(Date.UTC(2026, 8, 15, 1, 0, index)).toISOString(), kind: "input", text: `entry ${index}`, raw: {} };
}

function answer(url: URL): FeldApi.TranscriptPage {
  const limit = Number(url.searchParams.get("limit"));
  const before = url.searchParams.get("before");
  const after = url.searchParams.get("after");
  let records: FeldApi.TranscriptEntry[];
  let eligible: number;
  if (after !== null) {
    records = transcript.filter((record) => record.cursor > after).slice(0, limit);
    return { session: "s", state: "readable", detail: null, after, nextAfter: records.at(-1)?.cursor ?? after, previousBefore: null, hasOlder: false, records };
  }
  const earlier = before === null ? transcript : transcript.filter((record) => record.cursor < before);
  records = earlier.slice(-limit);
  eligible = earlier.length;
  return { session: "s", state: "readable", detail: null, after: null, nextAfter: before === null ? records.at(-1)?.cursor ?? null : null, previousBefore: records[0]?.cursor ?? null, hasOlder: eligible > records.length, records };
}

beforeEach(() => {
  requests.length = 0;
  transcript = Array.from({ length: 250 }, (_, index) => entry(index + 1));
  vi.useFakeTimers();
  vi.stubGlobal("window", globalThis);
  vi.stubGlobal("fetch", async (path: string) => {
    const url = new URL(path, "http://daemon");
    requests.push(`${url.pathname}?${url.searchParams}`);
    return { ok: true, status: 200, json: async () => answer(url) };
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("a held live column reads its tail once, follows it, and keeps no empty page", async () => {
  const { consoleKeys, holdConsole } = await import("./data.ts");
  const { readOf } = await import("./model.ts");
  const client = new QueryClient();
  const pages = () => client.getQueryData<InfiniteData<ConsolePage, FeldConsoleCursor>>(consoleKeys.pages("transcript", "s"))!;

  // Two holders of one Session read it once.
  const releaseConsole = holdConsole(client, "transcript", "s", true);
  const releaseLine = holdConsole(client, "transcript", "s", false);
  await vi.advanceTimersByTimeAsync(0);
  expect(requests).toEqual(["/v1/sessions/s/transcript?tail=true&limit=200"]);
  expect(readOf(pages(), null)).toMatchObject({ hasOlder: true, previousBefore: "051", nextAfter: "250" });

  // Nothing appended: the forward read answers empty and is not held, and the data the
  // Console drew stays the same object.
  const first = pages();
  await vi.advanceTimersByTimeAsync(1000);
  expect(requests.at(-1)).toBe("/v1/sessions/s/transcript?after=250&limit=200");
  expect(pages()).toBe(first);

  transcript.push(entry(251));
  await vi.advanceTimersByTimeAsync(1000);
  expect(pages().pages).toHaveLength(2);
  expect(readOf(pages(), null).rows).toHaveLength(201);

  // The page before is read in front of the ones held.
  const { InfiniteQueryObserver } = await import("@tanstack/react-query");
  const { consolePagesOptions } = await import("./data.ts");
  const observer = new InfiniteQueryObserver(client, consolePagesOptions("transcript", "s"));
  await observer.fetchPreviousPage();
  expect(requests.at(-1)).toBe("/v1/sessions/s/transcript?before=051&limit=200");
  expect(readOf(pages(), null)).toMatchObject({ hasOlder: false, nextAfter: "251" });
  expect(readOf(pages(), null).rows.map((row) => "cursor" in row && row.cursor)).toEqual(transcript.map((record) => record.cursor));

  // Once no holder follows it live, the column is no longer read.
  releaseConsole();
  const count = requests.length;
  await vi.advanceTimersByTimeAsync(5000);
  expect(requests).toHaveLength(count);
  releaseLine();
});

test("a held column nobody follows reads its tail only when asked", async () => {
  const { holdConsole, readTails } = await import("./data.ts");
  const client = new QueryClient();
  const session = { id: "s", actor: "default/a" };

  const release = holdConsole(client, "transcript", "s", false);
  await vi.advanceTimersByTimeAsync(5000);
  expect(requests).toEqual(["/v1/sessions/s/transcript?tail=true&limit=200"]);

  transcript.push(entry(251));
  readTails(client, [session]);
  await vi.advanceTimersByTimeAsync(5000);
  expect(requests).toEqual(["/v1/sessions/s/transcript?tail=true&limit=200", "/v1/sessions/s/transcript?after=250&limit=200"]);

  // A column the open Console follows is left to it.
  const releaseConsole = holdConsole(client, "transcript", "s", true);
  const count = requests.length;
  readTails(client, [session]);
  await vi.advanceTimersByTimeAsync(0);
  expect(requests).toHaveLength(count);
  releaseConsole();
  release();
});
