// The daemon rejects a bucket below a minute; the page asks for about 120 bins.
const MIN_BUCKET_SECONDS = 60;
const TARGET_BINS = 120;

function isoOf(instant: number) {
  return new Date(instant).toISOString();
}

function bucketSecondsFor(duration: number) {
  return Math.max(MIN_BUCKET_SECONDS, Math.round(duration / TARGET_BINS / 1000));
}

// Whether an interval overlaps the query's half-open range, as the daemon decides it.
// An interval without a start overlaps nothing.
function overlaps(query: FeldLiveQuery, start: string | null, end: string | null) {
  const from = start === null ? NaN : Date.parse(start);
  if (!Number.isFinite(from)) return false;
  return (query.end === null || from < query.end) && (end === null || query.start < Date.parse(end));
}

// A Sub Agent Session counts from its parent's start at the earliest, in Live and past
// ranges alike: an attached Session's transcript can name Sub Agents from before the
// attach. One that ended by then counts nowhere.
function fromParentStart(parent: FeldApi.Session, subAgent: FeldApi.SubAgentSession) {
  const start = Math.max(subAgent.start === null ? NaN : Date.parse(subAgent.start), Date.parse(parent.start));
  if (!Number.isFinite(start) || (subAgent.end !== null && Date.parse(subAgent.end) <= start)) return null;
  return isoOf(start);
}

// The Sub Agent Sessions of a parent that the range shows.
function subAgentsInRange(query: FeldLiveQuery, parent: FeldApi.Session, subAgents: FeldApi.SubAgentSession[]) {
  return subAgents.filter((subAgent) => overlaps(query, fromParentStart(parent, subAgent), subAgent.end));
}

// What the page draws from the reads that have answered: the Sessions, records, and
// Messages of the live Actors of the Workspaces shown, each Session followed by its Sub
// Agent Sessions in the range. A Session or Sub Agent Session whose metrics read has not
// answered has no metrics entry.
function compose(query: FeldLiveQuery, workspaces: string[], held: FeldRangeHeld): FeldLiveData {
  const shown = query.workspace === null ? workspaces : [query.workspace];
  const actors = shown.flatMap((workspace) => held.actors.get(workspace) || []);
  const live = new Set(actors.map((actor) => `${actor.workspace}/${actor.name}`));
  const isLive = (workspace: string, name: string) => live.has(`${workspace}/${name}`);
  const parents = [...held.sessions.values()]
    .filter((session) => isLive(session.workspace, session.actor))
    .sort((left, right) => left.start.localeCompare(right.start) || left.id.localeCompare(right.id));
  const sessions: (FeldApi.Session | FeldApi.SubAgentSession)[] = [];
  const metrics: FeldApi.SessionMetrics[] = [];
  for (const parent of parents) {
    for (const session of [parent, ...subAgentsInRange(query, parent, held.subAgents.get(parent.id) || [])]) {
      sessions.push(session);
      const projection = held.metrics.get(session.id);
      if (projection) metrics.push(projection);
    }
  }
  return {
    workspaces,
    actors,
    sessions,
    metrics,
    records: held.records.filter((record) => isLive(record.actor.workspace, record.actor.name)),
    messages: held.messages.filter((message) => isLive(message.to.workspace, message.to.name)),
    runtimeMetrics: held.runtimeMetrics,
    runtimes: held.runtimes,
    providers: held.providers,
    nodes: held.nodes,
  };
}

export { isoOf, bucketSecondsFor, overlaps, subAgentsInRange, compose };
