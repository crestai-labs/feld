# Metrics sources and aggregation

This document gives the rules for deriving Session metrics, Runtime metrics, and the Web UI's Metrics lane from a Session's [Telemetry](../concepts/glossary.md#telemetry). For each metric it defines the unit, time, aggregation, and display when missing, and the sources are defined per Runtime Adapter. How telemetry passes through the Collector, is attributed to a Session, and is stored as Telemetry is in [otel.md](otel.md); the list of metrics and the definitions of `status` / `source` are in [Session metrics](../concepts/glossary.md#session-metrics) in the glossary and in the [Metrics lane](../../guide/webui.md#metrics-lane) of the Web UI views; the API is `Metrics` in the [OpenAPI](../../guide/daemon-api.openapi.yaml).

## Contents

- [List of metrics](#list-of-metrics)
- [Sources](#sources)
  - [The four token kinds](#the-four-token-kinds)
  - [Adapter gauges](#adapter-gauges)
- [Time and temporality](#time-and-temporality)
- [Projection](#projection)
  - [series](#series)
  - [status and source](#status-and-source)
- [Aggregation](#aggregation)
- [Sub Agent](#sub-agent)
- [quota](#quota)
- [Metrics lane](#metrics-lane)
- [Update notifications](#update-notifications)

## List of metrics

| metric | Unit | Bin of one Session | Whole Session | Bin of multiple Sessions |
|---|---|---|---|---|
| token usage | tokens (the four kinds and `total`) | sum | sum | sum |
| estimated cost | USD | sum | sum | sum |
| API errors | count | sum | sum | sum |
| prompt cache hit rate | % | sum the numerator and denominator, then divide | same | same |
| API request latency p95 | ms | p95 of per-request duration | same | p95 over the requests of all Sessions combined |
| tool calls | count | sum | sum | sum |
| context window utilization | % and window size | latest value in the bin | latest value | maximum of each Session's latest value |
| active time | seconds | sum | sum | sum |
| 5-hour / 7-day quota | % and reset time | — | — | see [quota](#quota) |

## Sources

The Runtime Adapter enables the Runtime's OTel metrics and logs (event) exporters, and sends values not included in OTel to the same Collector as [Adapter gauges](#adapter-gauges). The source and extraction of each metric, the metrics that are `n/a` or `unavailable` depending on the Runtime, and the `source` names for `reported` are defined per Runtime.

- [Claude Code](adapter/claude-code.md#metrics)
- [Codex](adapter/codex.md#metrics)

core holds these extractions as per-Runtime rules. Rules exist for the Runtimes listed above; for any other Runtime, metrics and quota are `n/a` ("Feld has no metric rules for this Runtime").

A log record carries the instrument name in the body, the `event.name` attribute, or the OTLP `event_name`. Projection takes, in this order, a name that is in the `metrics` of the Runtime Adapter's manifest.

### The four token kinds

token usage is expressed in four kinds regardless of Runtime: `input`, `output`, `cache_read`, and `cache_creation`, and `total` is the sum of the four. The mapping from a Runtime's token types to the four kinds is defined per Runtime.

prompt cache hit rate is `cache_read / (input + cache_read + cache_creation)`. A bin whose denominator is 0 is empty.

### Adapter gauges

context window and quota are not included in the Runtime's OTel, so a component of the Runtime Adapter builds gauges from values the Runtime holds and sends them to the Collector with the [telemetry token](auth.md#session-token) in a header. The data point time is the time the component received the value, and no data point is built from a field whose value is `null` or from an absent window. quota gauges carry the account as an attribute ([quota](#quota)).

## Time and temporality

- A metric data point goes into the bin of its `timeUnixNano`. Its value is the amount over the interval from `startTimeUnixNano` to `timeUnixNano`
- An event goes into the bin of its occurrence time. The occurrence time is the log record's `timeUnixNano`, or `observedTimeUnixNano` if that is 0
- Telemetry that arrives late through Collector retries also goes into the bin of the data point's or event's time
- The Sums and histograms that are projected are delta. When the Runtime can choose the temporality, the Adapter specifies delta, and projection checks each data point's `aggregationTemporality`. For a Session containing a non-delta data point, that metric is `unavailable` ("Runtime telemetry is not delta")
- For a Session containing negative or non-finite values, that metric is `unavailable` ("Invalid Runtime telemetry value"), and for a Session containing undecodable Telemetry, every metric is `unavailable` with the same reason
- The Runtime sends metrics in batches per export interval. The Adapter sets the interval to 10 seconds (`OTEL_METRIC_EXPORT_INTERVAL=10000`), and the minimum `bucket` is 60 seconds

## Projection

### series

`GET /v1/sessions/{sessionId}/metrics?start&end&bucket&metric` cuts `[start, end)` into bins of `bucket` seconds and builds a series per metric from the data points and events whose time falls in the range. Bin boundaries are multiples of `bucket` from the epoch. When `bucket` is omitted, the range (or the whole Session, if the range is also omitted) is aggregated into one value by the "Whole Session" column of the [List of metrics](#list-of-metrics). A bin without a value is empty, and a reported 0 makes a bin of 0.

context window utilization has values only in bins that have `used_percent`, and `size` is accompanied by the latest value reported up to that bin. For a Session that continues across the start of the range, the latest value before the range is kept in the first bin of the range.

`GET /v1/runtimes/{runtime}/metrics?start&end&bucket&metric` builds the Runtime metrics series by [aggregating](#aggregation) the bins of the in-range Sessions of that Runtime, across non-deleted Actors in all Workspaces, per the Session's [account](#account), and attaches [quota](#quota). Sessions with an unknown account are combined into one series, and that series has no account.

A read that specifies `start` reads Telemetry received from 5 minutes before the start of the range (the tolerance for clock differences of the measuring Node) onward. Telemetry received earlier is read going back, newest first, 16 at a time, until both `used_percent` and `size` data points of the context window are found, and the latest values before the range are taken from there. Parts not in the Runtime's `metrics` are not searched.

### status and source

The `status` of each metric is decided in the following order.

1. If the Session's `execution.type` is `program`, `n/a`. Telemetry attribution happens only through the Runtime Adapter, and a program is outside it
2. If it is a Sub Agent Session, `n/a` ([Sub Agent](#sub-agent))
3. If [Sources](#sources) defines the metric as `n/a` or `unavailable` for that Runtime, that status. For a Runtime for which core has no rules, `n/a`
4. If the Session's Runtime started without a telemetry path, `unavailable`. This covers a Session started without a Collector ([otel.md](otel.md#startup-and-listen)) and a Session that attached a Runtime for which the Runtime Adapter's `match` or `prepare` answered `telemetryPrepared` as false, and `source` holds the reason. In the latter case, metrics taken from [Adapter gauges](#adapter-gauges) are outside this rule
5. If a rule in [Time and temporality](#time-and-temporality) applies, the `unavailable` defined there
6. If the range contains a data point or event from the source, `reported`, with a series
7. Otherwise, `unavailable` ("No correlated Runtime usage record")

`source` holds the grounds for `status` as a human-readable string: the source name for `reported`, otherwise the reason determined in the steps above.

For counting metrics (API errors, tool calls), when token usage is `reported` over the same range, bins that have token usage are counted as 0 even without a source, and the metric is `reported`. prompt cache hit rate is `reported` when token usage is `reported`, even if no bin has a value.

For context window utilization, in place of steps 6 and 7, it is `reported` if it has bins under the [series](#series) rules, and otherwise `unavailable` with "No correlated Runtime usage record".

## Aggregation

When combining multiple Sessions or bins, follow the rules in the [List of metrics](#list-of-metrics).

- Additive metrics (token usage, estimated cost, API errors, tool calls, active time) are summed
- prompt cache hit rate sums each of the four token kinds, then applies the formula
- API request latency p95 is computed from all source values within the combined range. For Runtimes that take it from a histogram, values are distributed linearly across bucket boundaries
- context window utilization takes, per Session, the data point with the latest time in the bin, and across multiple Sessions takes the maximum value. The Session the value was taken from is attached to the bin
- Sessions that are not `reported` are excluded from the combination, and the number of Sessions per status is attached

## account

A Session's account is decided in the following order. If there is no display name, the ID is used as the display name.

1. The account attributes of the Runtime's OTel in the Session's Telemetry. The value of the data point with the latest time is taken. The attributes per Runtime are as follows

   | Runtime | ID | Display name |
   |---|---|---|
   | Claude Code | `user.account_uuid` | `user.email` |
   | Codex | `user.account_id` | `user.email` |

2. The attributes `account.id` (ID) and `account.name` (display name) of the Session's quota [gauge](#adapter-gauges). The value of the data point with the latest time is taken. The component runs as a child process of the Runtime, has the same configuration root as the Runtime in its environment, and attaches the account that configuration root is logged in to
3. If neither exists, the account is unknown

The display name of a Runtime metrics series is taken from the Session with the latest start among the Sessions of the same account.

## Sub Agent

A Sub Agent uses the same exporter and the same telemetry token as its parent, inside the parent's Runtime process. A Sub Agent's telemetry is included in the parent Session's Telemetry and counted in the parent Session's metrics.

A Sub Agent Session has no metrics, and the daemon answers a read of a Sub Agent Session's metrics with every metric as `n/a` ("Included in the parent Session"). Since the parent Session's metrics include the Sub Agent's share, Runtime metrics and Actor totals do not count Sub Agent Sessions.

## quota

quota is a value belonging to an account and arrives as a Session's report. The windows that arrive depend on the Runtime and plan. The daemon does not store reports as aggregated values; it derives them per Runtime on a Runtime metrics read (`GET /v1/runtimes/{runtime}/metrics`).

- A report is a tuple of Runtime, `window`, time, `used_percent`, `resets_at`, and the reporting Session and its account
- The reports in scope are those of the same Sessions as the Runtime metrics in [series](#series)
- `status` and `source` are returned: `n/a` for a Runtime for which core has no rules, `reported` if there is a report in the range, otherwise `unavailable` ("No rate limit report"). Reports from a Session whose quota gauge has invalid values (negative, non-finite, `used_percent` over 100, or no `window`) or undecodable Telemetry are excluded from quota
- A series is built per combination of account ID, window, and `resets_at`, and each point is a report's value
- Each series is accompanied by the latest report and the Session it came from. A series of reports with a known account is also accompanied by the account (ID and display name)
- A series past its `resets_at` ends at that time

A report's account is the account in the reporting Session's Runtime OTel (item 1 of [account](#account)), or else the report gauge's `account.id` and `account.name`; if neither exists, the account is unknown. A series' display name is taken from the latest report.

The Web UI has one quota lane per combination of Runtime and window, with a line per account. The lane windows are 5-hour and 7-day for Claude Code, and 7-day for other Runtimes. For each account, its series are drawn as one line in reset order, and each series' value is held until its `resets_at`. The lane name shows the window length. `resets_at` is drawn as a reset marker.

## Metrics lane

The Web UI's Metrics lane reads `GET /v1/runtimes/{runtime}/metrics` for each installed Runtime and draws a lane per metric. Each lane has a line per combination of Runtime and account, and series with an unknown account are drawn as the Runtime's line. The lane's `derivation` shows the [Aggregation](#aggregation) rules. Bins without values are drawn as empty. context window values are held until the next value. In the histogram of the Actors band, a Session's last value is held until its end (now, for a running Session). The quota lanes are drawn from the quota of the same read. Display units and display names follow the Web UI implementation (`web/`).

A lane's reading shows, per line, the line's name and one of the following. Lines in quota lanes are named by the account's display name, and lines in other lanes by the Runtime name and the account's display name. Lines with an unknown account and Runtimes without series are shown by the Runtime name alone. A reading that has a sample is accompanied by that sample's time.

| Reading | Condition |
|---|---|
| value | The latest sample in the range. For level lanes (quota, context window), the value held at the end of the range |
| `Measured zero` | The latest sample in the range reported 0 |
| `0% since reset` | The quota window reset by the end of the range and there is no report after that. The time is the reset time |
| `No samples in range` | Telemetry is obtainable but there is no sample in the range: when there is no Session of that Runtime in the range, when a Session is `reported` but has no bins, or when the `source` of `unavailable` is "No correlated Runtime usage record" or "No rate limit report" |
| `Telemetry unavailable` | Every Session's metric is `unavailable` for a reason other than the above |
| `N/A` | Every Session's metric is `n/a` |

## Update notifications

The `GET /v1/events` SSE notifies only the type and ID of the changed resource. On a Session notification, the Web UI redoes `GET /v1/sessions/{sessionId}/metrics`, and when SSE is unavailable it calls the same read periodically. `GET /v1/runtimes/{runtime}/metrics` is reread independently of notifications, every 1/12 of the bin length (at least 5 seconds) in the Live range.
