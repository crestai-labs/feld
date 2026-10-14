import assert from "node:assert/strict";
import { test } from "vitest";
import { carriesTurn, lastTurnRecord, conversationItems, toolName, toolInput } from "./runtime-records.ts";

test("runtime-records", () => {
  let next = 0;
  const record = (kind, text = "") => ({ cursor: `c${(next += 1).toString().padStart(3, "0")}`, at: "2026-09-15T01:00:00Z", kind, text, raw: {} });

  // The turn counts inputs, responses, reasoning, tool calls, and tool results; it skips the
  // Runtime's own lines and whatever only the All events view shows.
  assert.equal(carriesTurn(record("input")), true);
  assert.equal(carriesTurn(record("notification")), true);
  assert.equal(carriesTurn(record("tool-call")), true);
  assert.equal(carriesTurn(record("tool-result")), true);
  assert.equal(carriesTurn(record("thinking")), true);
  assert.equal(carriesTurn(record("system")), false);
  assert.equal(carriesTurn(record("other")), false);
  assert.equal(carriesTurn(undefined), false);

  assert.equal(lastTurnRecord([]), null);
  assert.equal(lastTurnRecord([record("system"), record("other")]), null);
  // A `system` line or an All-events-only record after the response is skipped.
  assert.equal(lastTurnRecord([record("response"), record("system"), record("other")]).kind, "response");
  assert.equal(lastTurnRecord([record("response"), record("tool-call")]).kind, "tool-call");
  // A Sub Agent's records are its own, not the Session's.
  assert.equal(lastTurnRecord([record("input"), { ...record("response"), subAgent: "Explore" }]).kind, "input");

  // The Conversation view drops `other`, keeps `input` and `notification` apart, and joins
  // each tool call to the tool result that follows it.
  {
    const prompt = record("input", "delegate");
    const note = record("notification", "<task-notification>done</task-notification>");
    const answer = record("response", "on it");
    const call = record("tool-call", "Bash\n{\n  \"command\": \"ls\"\n}");
    const result = record("tool-result", "README.md");
    const skipped = record("other");
    const items = conversationItems([prompt, note, answer, call, result, skipped]);
    assert.equal(items.map((item) => item.kind).join(" "), "input notification response tool-call");
    assert.equal(items[3].result, result);
    assert.equal(toolName(call), "Bash");
    assert.equal(toolInput(call), "{\n  \"command\": \"ls\"\n}");
  }

  // Parallel calls take their results in the order they came, and a result whose call is
  // above the page held stands on its own.
  {
    const first = record("tool-call", "Read\n{}");
    const second = record("tool-call", "Grep\n{}");
    const firstResult = record("tool-result", "one");
    const secondResult = record("tool-result", "two");
    const items = conversationItems([first, second, firstResult, secondResult]);
    assert.equal(items.map((item) => item.kind).join(" "), "tool-call tool-call");
    assert.equal(items[0].result, firstResult);
    assert.equal(items[1].result, secondResult);

    const orphan = record("tool-result", "left over");
    const alone = conversationItems([orphan, record("response", "done")]);
    assert.equal(alone.map((item) => item.kind).join(" "), "tool-result response");
    assert.equal(alone[0].result, null);
  }

  // A call the Runtime has not answered yet keeps no result, which is what the tail of a
  // running Session looks like.
  {
    const call = record("tool-call", "Bash");
    const items = conversationItems([call]);
    assert.equal(items[0].result, null);
    assert.equal(toolName(call), "Bash");
    assert.equal(toolInput(call), "");
  }
});
