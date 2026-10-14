// The transcript entries the daemon answers with are already in the common shape a Runtime
// Adapter projects onto: `cursor`, `at`, `kind`, `text`, `subAgent`, `subAgentEnded`,
// `raw`. Everything the Console and the Actor line read from them is here, so the Web UI
// knows no Runtime's own line shape.

// The kinds the turn counts. A Runtime's own line (`system`) and a record only the All
// events view shows (`other`) carry no turn.
const TURN_KINDS = new Set<FeldApi.TranscriptKind | undefined>(["input", "notification", "response", "thinking", "tool-call", "tool-result"]);

function carriesTurn(record: FeldApi.TranscriptEntry | null | undefined) {
  return TURN_KINDS.has(record?.kind);
}

// The Session's own last record that carries a turn: a Sub Agent's records are its own.
function lastTurnRecord(records: readonly FeldApi.TranscriptEntry[]) {
  for (let index = records.length - 1; index >= 0; index -= 1) {
    if (!records[index].subAgent && carriesTurn(records[index])) return records[index];
  }
  return null;
}

// The Conversation view's rows: every record but `other`, with each tool call joined to
// the tool result that follows it. A result with no call before it stands on its own,
// which is what the tail of a paged Console looks like.
function conversationItems(records: readonly FeldApi.TranscriptEntry[]) {
  const items: FeldConversationItem[] = [];
  const unanswered: FeldConversationItem[] = [];
  for (const record of records) {
    if (record.kind === "other") continue;
    if (record.kind === "tool-result" && unanswered.length) {
      unanswered.shift()!.result = record;
      continue;
    }
    const item: FeldConversationItem = { kind: record.kind, record, result: null };
    if (record.kind === "tool-call") unanswered.push(item);
    items.push(item);
  }
  return items;
}

// A tool call's text names the tool on its first line and carries what it was called
// with below it.
function toolName(record: FeldApi.TranscriptEntry | null | undefined) {
  return String(record?.text || "").split("\n", 1)[0] || "tool";
}

function toolInput(record: FeldApi.TranscriptEntry | null | undefined) {
  const text = String(record?.text || "");
  const firstBreak = text.indexOf("\n");
  return firstBreak === -1 ? "" : text.slice(firstBreak + 1);
}

export {
  carriesTurn,
  lastTurnRecord,
  conversationItems,
  toolName,
  toolInput,
};
