// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createQueryClient } from "../api/queries.ts";
import { FeldRecordRow, RowsContext } from "./rows.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const WHOLE = "## Outcome\n\n- RESULT: DONE\n- TICKET: example-org/libra#22\n\nWaiting for the next message.";

function record(envelope: Partial<FeldConsoleEnvelope>): FeldConsoleRecord {
  return {
    id: "message_1", type: "message.sent", kind: "message-sent", at: 0, title: "Message sent", summary: "waiting in mailbox since 15:16",
    body: "## Outcome · RESULT: DONE · TICKET: example-org/libra#2…", anchor: null, conversation: true, facts: [], links: [],
    message: { id: "message_1", role: "To", actor: "dispatcher", sessionId: "session_D0", body: null, ...envelope },
  };
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

async function draw(item: FeldConsoleRecord) {
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <RowsContext.Provider value={{ sessionId: "session_D2", mark: null, timeZone: "UTC" }}>
        <FeldRecordRow item={{ record: item, host: null }} offset={0} />
      </RowsContext.Provider>
    </QueryClientProvider>,
  ));
}

test("a Message record names the other party on its own line and shows the body it holds, whole", async () => {
  vi.stubGlobal("fetch", vi.fn());
  await draw(record({ body: WHOLE }));
  const envelope = host.querySelector(".feld-console__feld-envelope")!;
  expect(envelope.querySelector("span")?.textContent).toBe("To");
  expect(envelope.textContent).toBe("Todispatcher");
  const body = host.querySelector(".feld-console__feld-body")!;
  expect(body.classList.contains("is-preview")).toBe(false);
  expect(body.textContent).toContain("Waiting for the next message.");
  expect(fetch).not.toHaveBeenCalled();
});

test("a Message record of the daemon's list shows the preview, then reads the Message for the whole body", async () => {
  let answer!: (value: unknown) => void;
  vi.stubGlobal("fetch", vi.fn(() => new Promise((resolve) => { answer = resolve; })));
  await draw(record({ body: null }));
  const body = () => host.querySelector(".feld-console__feld-body")!;
  expect(body().classList.contains("is-preview")).toBe(true);
  expect(body().textContent).toContain("TICKET: example-org/libra#2…");
  expect(vi.mocked(fetch).mock.calls[0][0]).toBe("/v1/messages/message_1");

  // The cache tells its observers on a later task.
  await act(async () => {
    answer({ ok: true, status: 200, headers: new Headers(), json: async () => ({ message: { id: "message_1", body: WHOLE } }) });
    await vi.waitFor(() => expect(body().classList.contains("is-preview")).toBe(false));
  });
  expect(body().textContent).toContain("Waiting for the next message.");
  expect(host.querySelector(".feld-console__feld-body-note")).toBeNull();
});

test("a short body has no toggle; a long one shows its head until opened, and the opening is kept", async () => {
  vi.stubGlobal("fetch", vi.fn());
  await draw(record({ role: "From", actor: "Admin", sessionId: null, body: "hello" }));
  expect(host.querySelector(".feld-console__feld-envelope")?.textContent).toBe("FromAdmin");
  expect(host.querySelector(".feld-console__feld-more")).toBeNull();

  const long = Array.from({ length: 20 }, (_, index) => `line ${index + 1}`).join("\n");
  await draw(record({ body: long }));
  const text = () => host.querySelector(".feld-console__feld-text")!;
  const more = () => host.querySelector<HTMLButtonElement>(".feld-console__feld-more")!;
  expect(text().classList.contains("is-clipped")).toBe(true);
  expect(more().textContent).toBe("Show the whole text · 20 lines");
  // The text is whole in the page either way; the clip is the frame's.
  expect(text().textContent).toContain("line 20");

  await act(async () => more().click());
  expect(text().classList.contains("is-clipped")).toBe(false);
  expect(more().textContent).toBe("Show less");

  // Drawn again, as a re-read Console draws it, the body stays open.
  await act(async () => root.unmount());
  root = createRoot(host);
  await draw(record({ body: long }));
  expect(text().classList.contains("is-clipped")).toBe(false);
});

test("a record's ledger opens from the facts button in its header, and its links stand in the header", async () => {
  vi.stubGlobal("fetch", vi.fn());
  const opened: string[] = [];
  const spawn: FeldConsoleRecord = {
    id: "action_spawn_T0", type: "spawned", kind: "spawn", at: 0, title: "Spawned test-runner", summary: "", anchor: null, conversation: true,
    facts: [{ label: "Spawn", value: "accepted", detail: "created session_T0", provenance: "Control Plane fact" }, { label: "Requester", value: "dispatcher · session_D0", detail: "", provenance: "Control Plane fact" }],
    links: [{ label: "Open session_T0", sessionId: "session_T0", recordId: "action_spawn_T0" }],
  };
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <RowsContext.Provider value={{ sessionId: "session_D0", mark: null, timeZone: "UTC", onOpenConsole: (sessionId, recordId) => opened.push(`${sessionId}:${recordId}`) }}>
        <FeldRecordRow item={{ record: spawn, host: null }} offset={0} />
      </RowsContext.Provider>
    </QueryClientProvider>,
  ));
  const header = host.querySelector(".feld-console__feld header")!;
  expect(header.querySelector(".feld-console__feld-summary")).toBeNull();
  expect(header.querySelector(".feld-console__feld-links button")?.textContent).toBe("Open session_T0 →");
  const facts = header.querySelector<HTMLButtonElement>(".feld-console__feld-facts")!;
  expect(facts.textContent).toBe("2 facts");
  expect(facts.getAttribute("aria-expanded")).toBe("false");
  expect(host.querySelector(".feld-console__feld-detail")).toBeNull();

  await act(async () => facts.click());
  expect(facts.getAttribute("aria-expanded")).toBe("true");
  expect(host.querySelector(".feld-console__feld-record")?.textContent).toBe("Record · spawnedaction_spawn_T0");
  expect([...host.querySelectorAll(".feld-console__ledger dt")].map((term) => term.textContent)).toEqual(["Spawn", "Requester"]);

  await act(async () => header.querySelector<HTMLButtonElement>(".feld-console__feld-links button")!.click());
  expect(opened).toEqual(["session_T0:action_spawn_T0"]);
});
