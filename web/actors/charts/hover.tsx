// The value tooltip of the band histograms and the Metrics charts, the Actor state glyph's
// tooltip, and the one-line tooltip of a Session's end mark. One tooltip follows the pointer
// over all of them; it is created the first time one shows it and then kept, so it fades in
// and out as the pointer comes and goes.

import { Fragment, useLayoutEffect, useRef, type ReactNode } from "react";
import { create } from "zustand";
import { formatDuration, formatMetricValue, metricValueAt, laneSeriesName, quotaReporterAt, seriesPlotted, seriesSampledAt, seriesStatusLabel, seriesTone, type ActorsModel } from "../model.ts";
import type { ActorState, AgentSession, Instant, ListedMetricLane, MetricSeries } from "../types.ts";

interface ChartTooltipState {
  content: ReactNode;
  clientX: number;
  clientY: number;
  visible: boolean;
  show: (content: ReactNode, clientX: number, clientY: number) => void;
  hide: () => void;
}

export const useChartTooltip = create<ChartTooltipState>((set) => ({
  content: null,
  clientX: 0,
  clientY: 0,
  visible: false,
  show: (content, clientX, clientY) => set({ content, clientX, clientY, visible: true }),
  hide: () => set((state) => (state.visible ? { visible: false } : state)),
}));

// The tooltip sits beside the pointer, above it and to its right, and flips to the other
// side where the viewport has no room. A tooltip taller or wider than the room on either
// side is kept inside the viewport.
export function ChartTooltip() {
  const { content, clientX, clientY, visible } = useChartTooltip();
  const ref = useRef<HTMLDivElement>(null);
  const shown = useRef(false);
  if (visible) shown.current = true;
  useLayoutEffect(() => {
    const tooltip = ref.current;
    if (!tooltip) return;
    if (!visible) {
      tooltip.classList.remove("is-visible");
      return;
    }
    const offset = 14;
    const width = tooltip.offsetWidth;
    const height = tooltip.offsetHeight;
    const left = clientX + offset + width > window.innerWidth - 8 ? clientX - offset - width : clientX + offset;
    const top = clientY - offset - height < 8 ? clientY + offset : clientY - offset - height;
    tooltip.style.left = `${Math.max(8, Math.min(left, window.innerWidth - 8 - width))}px`;
    tooltip.style.top = `${Math.max(8, Math.min(top, window.innerHeight - 8 - height))}px`;
    tooltip.classList.add("is-visible");
  }, [content, clientX, clientY, visible]);
  if (!shown.current) return null;
  return <div className="feld-agents__chart-tooltip" data-chart-tooltip="" role="tooltip" ref={ref}>{content}</div>;
}

// The pointer's place in the viewBox of a chart.
export function viewPointFor(svg: SVGSVGElement, clientX: number, clientY: number) {
  const rect = svg.getBoundingClientRect();
  const view = svg.viewBox.baseVal;
  return {
    x: (clientX - rect.left) * (view.width || rect.width) / Math.max(rect.width, 1),
    y: (clientY - rect.top) * (view.height || rect.height) / Math.max(rect.height, 1),
  };
}

function TooltipRow({ label, value, tone }: { label: string; value: string; tone: string | null }) {
  return (
    <span className="feld-agents__chart-tooltip-row">
      {tone ? <span className={`feld-agents__metric-swatch is-${tone}`} aria-hidden="true"></span> : null}
      <span>{label}</span>
      <strong>{value}</strong>
    </span>
  );
}

// The one-line tooltip of a mark: what the mark says, and why.
export function markTooltip(label: string, reason: string) {
  return <span className="feld-agents__chart-tooltip-line"><strong>{label}</strong>{` · ${reason}`}</span>;
}

// The Actor state glyph's tooltip: the Schedule, then the current state and since when.
export function actorStateTooltip({ lines: [schedule, now] }: ActorState) {
  return (
    <>
      <span className="feld-agents__chart-tooltip-title">{schedule}</span>
      <span>{now}</span>
    </>
  );
}

// A histogram bar's box, where the pointer hovers it, and what it reads.
export interface HistogramHit {
  left: number;
  right: number;
  top: number;
  bottom: number;
  session: AgentSession;
  bin: { start: Instant; end: Instant; title: string };
  binDuration: number;
}

export function histogramTooltip(model: ActorsModel, { session, bin, binDuration }: HistogramHit) {
  return (
    <>
      <span className="feld-agents__chart-tooltip-title">{bin.title}</span>
      <span className="feld-agents__chart-tooltip-meta">{session.subAgent ? `${session.id} · ${session.subAgent}` : session.id}</span>
      <span className="feld-agents__chart-tooltip-meta">{`${model.timeOnly.format(bin.start)}–${model.timeOnly.format(bin.end)} ${model.ZONE} · ${formatDuration(binDuration)} bin`}</span>
    </>
  );
}

// The values of the series the lane draws, at the hovered instant, highest first.
export function metricTooltip(model: ActorsModel, lane: ListedMetricLane, instant: Instant) {
  const { metric } = lane;
  const valueAt = (series: MetricSeries) => (seriesSampledAt(metric, series, instant) ? metricValueAt(metric, series, instant)! : -1);
  const series = metric.kind === "quota" ? [...lane.series].sort((left, right) => valueAt(right) - valueAt(left)) : lane.series;
  const rows = series.map((series, index) => {
    const name = laneSeriesName(metric, series);
    const tone = seriesTone(series);
    if (!seriesPlotted(series)) return <TooltipRow label={name} value={seriesStatusLabel(series)} tone={tone} key={index} />;
    if (!seriesSampledAt(metric, series, instant)) return <TooltipRow label={name} value="No sample at this time" tone={tone} key={index} />;
    const reporter = metric.kind === "quota" ? quotaReporterAt(series, instant) : null;
    const value = metricValueAt(metric, series, instant)!;
    return (
      <Fragment key={index}>
        <TooltipRow label={name} value={value === 0 ? "Measured zero" : formatMetricValue(metric, value)} tone={tone} />
        {reporter ? <span className="feld-agents__chart-tooltip-meta is-indented">latest report from {reporter.id}</span> : null}
      </Fragment>
    );
  });
  return (
    <>
      <span className="feld-agents__chart-tooltip-title">{metric.name}</span>
      {rows}
      <span className="feld-agents__chart-tooltip-meta">{`${model.dateTime.format(instant)} ${model.ZONE}`}</span>
    </>
  );
}
