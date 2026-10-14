// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createQueryClient } from "../api/queries.ts";
import { ConsoleTerminal } from "./ConsoleTerminal.tsx";
import { consoleKeys } from "./data.ts";
import { recordingTime } from "./model.ts";
import { forgetConsoles } from "./store.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Every Message and All events row shows its time, once for each time it is drawn.
vi.mock("./model.ts", async (original) => {
  const model = await original<typeof import("./model.ts")>();
  return { ...model, recordingTime: vi.fn(model.recordingTime) };
});
const drawn = vi.mocked(recordingTime);

describe("the pages of a transcript", () => {
  // The daemon as the Console reads it: a transcript of more than one page, every request
  // made, and whether a read of the page before fails. The page has no layout, so every
  // column is too short to scroll.
  const requests: string[] = [];
  let transcript: FeldApi.TranscriptEntry[] = [];
  let olderFails = false;

  function entry(index: number): FeldApi.TranscriptEntry {
    return { cursor: String(index).padStart(3, "0"), at: new Date(Date.UTC(2026, 8, 15, 1, 0, index)).toISOString(), kind: "input", text: `entry ${index}`, raw: {} };
  }

  function answer(url: URL): FeldApi.TranscriptPage {
    const limit = Number(url.searchParams.get("limit"));
    const before = url.searchParams.get("before");
    const after = url.searchParams.get("after");
    if (after !== null) {
      const records = transcript.filter((record) => record.cursor > after).slice(0, limit);
      return { session: "s", state: "readable", detail: null, after, nextAfter: records.at(-1)?.cursor ?? after, previousBefore: null, hasOlder: false, records };
    }
    const earlier = before === null ? transcript : transcript.filter((record) => record.cursor < before);
    const records = earlier.slice(-limit);
    return { session: "s", state: "readable", detail: null, after: null, nextAfter: before === null ? records.at(-1)?.cursor ?? null : null, previousBefore: records[0]?.cursor ?? null, hasOlder: earlier.length > records.length, records };
  }

  const olderReads = () => requests.filter((request) => request.includes("before="));

  beforeEach(() => {
    requests.length = 0;
    transcript = Array.from({ length: 250 }, (_, index) => entry(index + 1));
    olderFails = false;
    vi.useFakeTimers();
    vi.stubGlobal("fetch", async (path: string) => {
      const url = new URL(path, "http://daemon");
      requests.push(`${url.pathname}?${url.searchParams}`);
      if (url.pathname.endsWith("/records")) return { ok: true, status: 200, json: async () => ({ records: [], nextAfter: null }) };
      if (olderFails && url.searchParams.has("before")) return { ok: false, status: 500, statusText: "Internal Server Error", json: async () => ({ message: "the page before cannot be read" }) };
      return { ok: true, status: 200, json: async () => answer(url) };
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  async function open(status: string) {
    const page = document.createElement("div");
    document.body.append(page);
    const root = createRoot(page);
    await act(async () => root.render(
      <QueryClientProvider client={new QueryClient()}>
        <ConsoleTerminal session={{ id: "s", actor: "reviewer" }} actorLabel="reviewer" feldRecords={[]} timeOrigin={null} sessionStart={0} status={status} statusLabel={status} statusTitle={status} timeZone="UTC" />
      </QueryClientProvider>,
    ));
    return { page, close: () => act(async () => root.unmount()) };
  }

  const settle = (ms = 0) => act(() => vi.advanceTimersByTimeAsync(ms));

  test("a column too short to scroll reads the pages before without a scroll", async () => {
    const { page, close } = await open("exited");
    await settle();
    expect(olderReads()).toEqual(["/v1/sessions/s/transcript?before=051&limit=200"]);
    expect(page.querySelector(".feld-console__older")).toBeNull();
    expect(page.querySelectorAll("[data-recording-panel='conversation'] [data-runtime-record]").length).toBe(250);
    await close();
  });

  test("a failed read of the page before waits to be asked while the tail is read again", async () => {
    olderFails = true;
    const { page, close } = await open("running");
    await settle();
    expect(olderReads()).toHaveLength(1);

    // The tail grows and is read again, which clears the query's error.
    for (let step = 0; step < 3; step += 1) {
      transcript.push(entry(transcript.length + 1));
      await settle(1000);
    }
    expect(requests.filter((request) => request.includes("after="))).not.toHaveLength(0);
    expect(olderReads()).toHaveLength(1);

    // Pressing the head of the column asks again.
    olderFails = false;
    const older = page.querySelector<HTMLButtonElement>("button.feld-console__older")!;
    await act(async () => older.click());
    await settle();
    expect(olderReads()).toHaveLength(2);
    expect(page.querySelector(".feld-console__older")).toBeNull();
    await close();
  });
});

describe("the output of a live program", () => {
  const SESSION = "session_program";
  const START = Date.parse("2026-10-11T00:00:00Z");

  let client: QueryClient;
  let host: HTMLElement;
  let root: Root;

  function line(sequence: number): FeldApi.OutputLine {
    return { sequence, stream: "stdout", timestamp: new Date(START + sequence * 1000).toISOString(), text: `line ${sequence}` };
  }

  // The output the daemon answered: one page holding `lines`, the whole column.
  function answer(lines: FeldApi.OutputLine[]) {
    const page = { session: SESSION, lines, hasOlder: false, previousBefore: null, nextAfter: lines.at(-1)?.sequence ?? null } as unknown as FeldApi.OutputPage;
    // The cache tells its observers on the next task.
    return act(async () => {
      client.setQueryData(consoleKeys.pages("output", SESSION), { pages: [page], pageParams: [{ limit: 200 }] });
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }

  function told() {
    const status = host.querySelector(".feld-console__announcement");
    if (!status) throw new Error("the status is drawn");
    return status.textContent;
  }

  beforeEach(async () => {
    // Every read waits: the test answers them through the query cache.
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
    client = createQueryClient();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(
      <QueryClientProvider client={client}>
        <ConsoleTerminal
          session={{ id: SESSION, actor: "job", console: "log" }}
          actorLabel="job"
          feldRecords={[]}
          timeOrigin={null}
          sessionStart={START}
          status="running"
          statusLabel="Running"
          statusTitle=""
          timeZone="UTC"
        />
      </QueryClientProvider>,
    ));
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  test("the transcript is no live region; the lines added at the tail are told as a count", async () => {
    expect(host.querySelector(".feld-console__transcript")?.getAttribute("aria-live")).toBe("off");
    await answer([line(1), line(2)]);
    expect(told()).toBe("");
    await answer([line(1), line(2), line(3), line(4), line(5)]);
    expect(told()).toBe("3 lines added");
  });

  test("the first lines of an output read empty are told", async () => {
    expect(told()).toBe("");
    await answer([]);
    await answer([line(1)]);
    expect(told()).toBe("1 line added");
  });
});

describe("the drawing of a long Console", () => {
  // The daemon as the Console reads it: a running Session's transcript that grows.
  let transcript: FeldApi.TranscriptEntry[] = [];

  function entry(index: number, kind: FeldApi.TranscriptKind = index % 2 ? "input" : "response"): FeldApi.TranscriptEntry {
    return { cursor: String(index).padStart(3, "0"), at: new Date(Date.UTC(2026, 8, 15, 1, 0, index)).toISOString(), kind, text: kind === "tool-call" ? `Read\n{"file":"${index}"}` : `entry ${index}`, raw: { index } };
  }

  function answer(url: URL) {
    if (url.pathname === "/v1/records") return { records: [], nextAfter: null };
    if (url.pathname.endsWith("/output")) {
      const lines = ["build ok", "test failed", "build done"].map((text, index) => ({ sequence: index + 1, timestamp: new Date(Date.UTC(2026, 8, 15, 1, 0, index)).toISOString(), stream: index === 1 ? "stderr" : "stdout", text }));
      return { session: "s", after: null, nextAfter: 3, previousBefore: null, hasOlder: false, lines };
    }
    const after = url.searchParams.get("after");
    const records = after === null ? transcript : transcript.filter((record) => record.cursor > after);
    return { session: "s", state: "readable", detail: null, after, nextAfter: records.at(-1)?.cursor ?? after, previousBefore: null, hasOlder: false, records };
  }

  beforeEach(() => {
    transcript = [...Array.from({ length: 40 }, (_, index) => entry(index + 1)), entry(41, "tool-call"), entry(42, "tool-result"), entry(43)];
    vi.useFakeTimers();
    vi.stubGlobal("fetch", async (path: string) => {
      const url = new URL(path, "http://daemon");
      return { ok: true, status: 200, json: async () => answer(url) };
    });
  });

  // A Console left open would keep holding its Session's column for the next test.
  const opened: Root[] = [];

  afterEach(async () => {
    await act(async () => opened.splice(0).forEach((root) => root.unmount()));
    vi.useRealTimers();
    vi.unstubAllGlobals();
    forgetConsoles();
  });

  // The Actors page hands the Console new objects with every drawing, equal to the last.
  function terminal(source?: FeldConsoleSession["console"]) {
    return (
      <ConsoleTerminal
        session={{ id: "s", actor: "default/a", model: "model", placement: "tmux", console: source } as FeldConsoleSession}
        actorLabel="a"
        feldRecords={[]}
        timeOrigin={null}
        sessionStart={Date.UTC(2026, 8, 15, 1)}
        status="running"
        statusLabel="Running"
        statusTitle=""
        timeZone="UTC"
        onOpenConsole={() => {}}
        onOpenJob={() => {}}
      />
    );
  }

  async function openConsole() {
    const client = new QueryClient();
    const page = document.createElement("div");
    document.body.append(page);
    const root = createRoot(page);
    opened.push(root);
    const draw = () => act(async () => root.render(<QueryClientProvider client={client}>{terminal()}</QueryClientProvider>));
    await draw();
    await act(() => vi.advanceTimersByTimeAsync(10));
    expect(page.querySelectorAll("[data-recording-panel='conversation'] > [data-recording-item]")).toHaveLength(42);
    return { page, draw };
  }

  // One step of following the tail, and the drawing its answer brings.
  const tick = async () => {
    await act(() => vi.advanceTimersByTimeAsync(1000));
    await act(() => vi.advanceTimersByTimeAsync(10));
  };

  test("a tick with nothing new draws no row again, and a new entry draws only its own row", async () => {
    const { page, draw } = await openConsole();

    drawn.mockClear();
    await tick();
    await draw();
    await tick();
    expect(drawn).not.toHaveBeenCalled();

    transcript.push(entry(44));
    await tick();
    expect(page.querySelectorAll("[data-recording-panel='conversation'] > [data-recording-item]")).toHaveLength(43);
    expect(drawn).toHaveBeenCalledTimes(1);
  });

  test("opening one row draws no other row, and what a ledger opens onto is drawn while it is open", async () => {
    const { page } = await openConsole();
    const work = page.querySelector<HTMLDetailsElement>(".feld-console__work")!;
    expect(work.querySelector(".feld-console__work-list")).toBeNull();

    drawn.mockClear();
    await act(async () => {
      work.open = true;
      work.dispatchEvent(new Event("toggle"));
    });
    expect(work.querySelector(".feld-console__work-list [data-runtime-record='041']")).not.toBeNull();
    expect(drawn).not.toHaveBeenCalled();
  });

  test("the transcript's views are offered from the start", async () => {
    const { page } = await openConsole();
    const chrome = page.querySelector<HTMLElement>(".feld-console__recording-chrome")!;
    expect(chrome.hidden).toBe(false);
    expect([...chrome.querySelectorAll("[data-recording-action='view']")].map((tab) => tab.textContent)).toEqual(["Conversation", "All events 43"]);
    expect(page.querySelector(".feld-console__terminal-actions")!.children).toHaveLength(0);
  });

  test("the view not shown draws none of its rows", async () => {
    const { page } = await openConsole();
    expect(page.querySelector("[data-recording-panel='events']")!.children).toHaveLength(0);

    await act(async () => page.querySelector<HTMLButtonElement>("[data-view='events']")!.click());
    expect(page.querySelector("[data-recording-panel='conversation']")!.children).toHaveLength(0);
    expect(page.querySelectorAll("[data-recording-panel='events'] > [data-recording-item]")).toHaveLength(43);
  });

  test("a program's output shows every line, with no tools over it", async () => {
    const client = new QueryClient();
    const page = document.createElement("div");
    document.body.append(page);
    const root = createRoot(page);
    opened.push(root);
    await act(async () => root.render(<QueryClientProvider client={client}>{terminal("log")}</QueryClientProvider>));
    await act(() => vi.advanceTimersByTimeAsync(10));
    expect([...page.querySelectorAll<HTMLElement>(".feld-console__log-line pre")].map((line) => line.textContent)).toEqual(["build ok", "test failed", "build done"]);
    expect(page.querySelector(".feld-console__recording-chrome")).toBeNull();
  });
});

describe("a transcript file not created yet", () => {
  afterEach(() => vi.unstubAllGlobals());

  // The notice of a live Session whose transcript file is not there yet, while its read waits.
  async function notice(session: FeldConsoleSession) {
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(
      <QueryClientProvider client={new QueryClient()}>
        <ConsoleTerminal session={session} actorLabel="dev" feldRecords={[]} timeOrigin={null} sessionStart={0} status="running" statusLabel="Running" statusTitle="" timeZone="UTC" />
      </QueryClientProvider>,
    ));
    const text = host.querySelector(".feld-console__unavailable")?.textContent;
    await act(async () => root.unmount());
    return text;
  }

  test("names the Session's Runtime as the one that creates the file", async () => {
    expect(await notice({ id: "s", actor: "dev", console: "missing", runtimeName: "Codex" })).toBe("The transcript file's location has arrived; Codex creates the file at the first input.");
    expect(await notice({ id: "s", actor: "dev", console: "missing" })).toBe("The transcript file's location has arrived; the Runtime creates the file at the first input.");
  });
});
