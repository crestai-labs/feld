function instantOf(value: string | null | undefined) {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

// The accounts a Runtime's reports of a window name, in the order the read lists them;
// null stands for the reports whose account is not known.
function quotaAccounts(runtime: FeldLiveQuota | null | undefined, window: FeldApi.QuotaSeries["window"]) {
  const accounts = new Map<string | null, FeldApi.Account | null>();
  for (const item of runtime?.series || []) {
    if (item.window !== window || !item.points?.length) continue;
    const id = item.account?.id ?? null;
    if (!accounts.has(id)) accounts.set(id, item.account ? { id: item.account.id, name: item.account.name || item.account.id } : null);
  }
  return [...accounts.values()];
}

// The quota read returns one series per account, window, and resets_at: a window that
// reset inside the range comes back as several series in a row, each ending at its own
// resets_at. The lane draws an account's as one line, so every series of the window and
// the account (null: the reports without one) is taken, in reset order, and each reading
// remembers the reset that ends it.
function quotaLane(runtime: FeldLiveQuota | null | undefined, window: FeldApi.QuotaSeries["window"], account: string | null = null): FeldQuotaLane | null {
  const series = (runtime?.series || [])
    .filter((item) => item.window === window && (item.account?.id ?? null) === account && item.points?.length)
    .map((item) => ({
      until: instantOf(item.end) ?? instantOf(item.resetsAt),
      resetsAt: instantOf(item.resetsAt),
      reporter: item.reportedBy || null,
      points: item.points
        .map((point) => ({ at: instantOf(point.at), value: Number(point.usedPercent || 0) }))
        .filter((point): point is { at: number; value: number } => point.at !== null)
        .sort((left, right) => left.at - right.at),
    }))
    .filter((item) => item.points.length)
    .sort((left, right) => (left.until ?? Infinity) - (right.until ?? Infinity) || left.points[0].at - right.points[0].at);
  if (!series.length) return null;
  const samples: FeldMetricSample[] = [];
  const segments: FeldQuotaSegment[] = [];
  const resets = new Set<number>();
  for (const item of series) {
    segments.push({ start: item.points[0].at, end: item.until, reporter: item.reporter });
    if (item.resetsAt !== null) resets.add(item.resetsAt);
    for (const point of item.points) {
      samples.push({ at: point.at, end: point.at, until: item.until, value: point.value });
    }
  }
  samples.sort((left, right) => left.at - right.at);
  return { samples, segments, resets: [...resets].sort((left, right) => left - right) };
}

// A level reading holds until the next one. A reading that carries the reset ending its
// series holds only until that reset: past it the window has reset, and its usage is 0
// until the next report.
function levelReadingAt(samples: readonly FeldMetricSample[], instant: number) {
  if (!samples.length) return 0;
  if (instant < samples[0].at) return samples[0].value;
  let reading = samples[0];
  for (const sample of samples) {
    if (sample.at > instant) break;
    reading = sample;
  }
  return reading.until != null && instant >= reading.until ? 0 : reading.value;
}

// The latest sample of a series over the range and what it reads: a rate series reads
// its latest bin in the range, a level series the reading it holds at the range's end.
// A level reading whose window reset before the range's end reads 0 from that reset,
// which is not a measurement: it carries `resetAt` instead. Null when no sample fell in
// the range.
function latestReading(samples: readonly FeldMetricSample[], range: FeldMetricRange, level?: boolean): FeldMetricReading | null {
  const latest = samples.filter((sample) => sample.at < range.end && (level || sample.end > range.start)).at(-1);
  if (!latest) return null;
  if (level && latest.until != null && latest.until < range.end) return { at: latest.until, end: latest.until, value: 0, resetAt: latest.until };
  return { at: latest.at, end: latest.end, value: level ? levelReadingAt(samples, range.end - 1) : latest.value };
}

function resetsWithin(resets: readonly number[] | null | undefined, range: FeldMetricRange) {
  return (resets || []).filter((reset) => reset >= range.start && reset < range.end);
}

// The Session named as the latest reporter of the series an instant falls in.
function reporterAt(segments: readonly FeldQuotaSegment[] | null | undefined, instant: number) {
  if (!segments?.length) return null;
  let reporter = segments[0].reporter;
  for (const segment of segments) {
    if (segment.start > instant) break;
    reporter = segment.reporter;
  }
  return reporter;
}

export { quotaAccounts, quotaLane, levelReadingAt, latestReading, resetsWithin, reporterAt };
