// The Metrics lanes under the Actors table: one lane per visible metric over the Metrics
// window, each series (a Runtime's, or a quota account's) as a line with its latest
// reading marked, the label column with every listed series' reading, and the folded
// header that keeps those readings. A window wider than the Timeline's veils what lies
// outside the Timeline's range.

import { Fragment, memo, useCallback, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useShallow } from "zustand/react/shallow";
import { useActorsModel } from "../context.ts";
import {
  METRIC_CHART_INSET,
  PLOT_PAD_LEFT,
  PLOT_PAD_RIGHT,
  gridLines,
  laneReading,
  metricReadingTitle,
  metricSampleTimes,
  metricTitle,
  metricValueAt,
  plotX,
  readingMarkerAt,
  laneSeriesName,
  seriesPlotted,
  seriesSampledAt,
  seriesTone,
  type ActorsModel,
} from "../model.ts";
import { useActorsStore } from "../store.ts";
import type { AgentsState, Instant, ListedMetricLane, MetricLane, MetricSeries } from "../types.ts";
import { metricTooltip, useChartTooltip, viewPointFor } from "./hover.tsx";
import { useCompactLayout, useParentBox, usePlotHandlers, type PointerAt } from "./interaction.ts";

export type MetricsView = Pick<AgentsState, "mode" | "anchor" | "duration" | "metricsDuration" | "visibleMetricIds" | "query" | "expandedMetricIds" | "emphasizedSeries">;

function selectMetricsView(state: AgentsState): MetricsView {
  return {
    mode: state.mode,
    anchor: state.anchor,
    duration: state.duration,
    metricsDuration: state.metricsDuration,
    visibleMetricIds: state.visibleMetricIds,
    query: state.query,
    expandedMetricIds: state.expandedMetricIds,
    emphasizedSeries: state.emphasizedSeries,
  };
}

// One swatch per Runtime, muted where none of its series is plotted: the accounts of a
// Runtime share its colour.
function MetricSwatches({ metric }: { metric: MetricLane }) {
  const byTone = new Map<string, boolean>();
  metric.series.forEach((series) => byTone.set(series.tone, byTone.get(series.tone) || seriesPlotted(series)));
  return (
    <span className="feld-agents__metric-swatches" aria-hidden="true">
      {[...byTone].map(([tone, plotted]) => (
        <span className={`feld-agents__metric-swatch is-${tone}${plotted ? "" : " is-unavailable"}`} key={tone}></span>
      ))}
    </span>
  );
}

// A series' key in its lane, shared by its reading row and its marks on the chart.
function seriesKey(metric: MetricLane, series: MetricSeries) {
  return `${metric.id}:${metric.series.indexOf(series)}`;
}

// Pointing at a reading row brings its line forward and lets the lane's other lines
// recede, so one account can be followed among several of the same colour.
function MetricReading({ model, metric, series, range }: { model: ActorsModel; metric: MetricLane; series: MetricSeries; range: FeldMetricRange }) {
  const reading = model.metricReadingFor(metric, series, range);
  const key = seriesKey(metric, series);
  const emphasize = useActorsStore.getState().emphasizeSeries;
  return (
    <span className={`feld-agents__metric-reading-row is-${reading.state}`} title={reading.title} data-metric-series={key} onPointerEnter={() => emphasize(key)} onPointerLeave={() => emphasize(null)}>
      <i className={`feld-agents__metric-dot is-${seriesTone(series)}`} aria-hidden="true"></i>
      <span className="feld-agents__metric-runtime">{laneSeriesName(metric, series)}</span>
      <span className="feld-agents__metric-reading">{reading.text}</span>
      <small className="feld-agents__metric-reset-at">{reading.resetTime}</small>
      <small className="feld-agents__metric-sample-at">{reading.sampleTime}</small>
    </span>
  );
}

// The readings of the series a lane lists, as the label column and the folded header show
// them, and the button that lists the folded ones or folds them again.
function MetricReadings({ model, lane, range }: { model: ActorsModel; lane: ListedMetricLane; range: FeldMetricRange }) {
  const { metric } = lane;
  const foldable = lane.hiddenCount > 0 || lane.expanded;
  const label = lane.expanded ? "Show fewer" : `+${lane.hiddenCount} more ${metric.kind !== "quota" ? "series" : lane.hiddenCount === 1 ? "account" : "accounts"}`;
  return (
    <output data-metric-value={metric.id}>
      {lane.series.map((series, index) => <MetricReading model={model} metric={metric} series={series} range={range} key={index} />)}
      {foldable ? (
        <button type="button" className="feld-agents__metric-more" data-action="toggle-metric-series" data-metric-id={metric.id} aria-expanded={lane.expanded} onClick={() => useActorsStore.getState().toggleMetricSeries(metric.id)}>{label}</button>
      ) : null}
    </output>
  );
}

function useMetricsView() {
  const model = useActorsModel();
  const view = useActorsStore(useShallow(selectMetricsView));
  const compact = useCompactLayout();
  return { model, view, range: model.metricsRangeFor(view), lanes: model.metricLanesFor(view, compact) };
}

// The label column: the contents of `.feld-agents__metric-labels`.
export const MetricLabels = memo(function MetricLabels() {
  const { model, range, lanes } = useMetricsView();
  return (
    <>
      {lanes.map((lane) => (
        <div className="feld-agents__metric-label" style={{ height: `${lane.height}px` }} key={lane.metric.id}>
          <span className="feld-agents__metric-identity" title={metricTitle(lane.metric)}>
            <MetricSwatches metric={lane.metric} />
            <span className="feld-agents__metric-name">
              <strong>{lane.metric.name}</strong>
              <small>{lane.metric.caption}</small>
            </span>
          </span>
          <MetricReadings model={model} lane={lane} range={range} />
        </div>
      ))}
    </>
  );
});

// Folded Metrics header: one item per visible lane with its swatches, name, and the
// same readings the unfolded label column shows, so the fold hides history, not values.
// The contents of `.feld-agents__metrics-summary`.
export const MetricsSummary = memo(function MetricsSummary() {
  const { model, range, lanes } = useMetricsView();
  if (!lanes.length) return <span className="feld-agents__metrics-summary-empty">No metrics selected</span>;
  return (
    <>
      {lanes.map((lane) => (
        <span className="feld-agents__metrics-summary-item" title={metricTitle(lane.metric)} key={lane.metric.id}>
          <MetricSwatches metric={lane.metric} />
          <span className="feld-agents__metrics-summary-name">{lane.metric.name}</span>
          <MetricReadings model={model} lane={lane} range={range} />
        </span>
      ))}
    </>
  );
});

export interface MetricsLayers {
  width: number;
  height: number;
  range: FeldMetricRange;
  lanes: ListedMetricLane[];
  marks: ReactNode;
}

export function metricsLayers(model: ActorsModel, view: MetricsView, width: number, compact = false): MetricsLayers {
  const lanes = model.metricLanesFor(view, compact);
  const range = model.metricsRangeFor(view);
  const height = lanes.reduce((total, lane) => total + lane.height, 0);
  const x = (instant: Instant) => plotX(range, width, instant);
  const usable = width - PLOT_PAD_LEFT - PLOT_PAD_RIGHT;
  // The vertical grid at the labelled ticks only: the lines of the lanes carry the detail.
  const marks: ReactNode[] = usable > 0 ? gridLines(range, usable, model.TIME_ZONE, { minor: false }).map(({ instant }) => (
    <line className="feld-agents__grid-line is-major" x1={x(instant).toFixed(1)} y1="0" x2={x(instant).toFixed(1)} y2={height} key={`grid:${instant}`} />
  )) : [];

  lanes.forEach((listed, index) => {
    const { metric, top: laneTop } = listed;
    const chartTop = laneTop + METRIC_CHART_INSET.top;
    const chartBottom = laneTop + listed.height - METRIC_CHART_INSET.bottom;
    const chartHeight = chartBottom - chartTop;
    const y = (value: number) => chartBottom - value / metric.maximum * chartHeight;
    const lane: ReactNode[] = [];
    if (index > 0) {
      lane.push(<line className="feld-agents__metric-separator" x1="0" y1={laneTop} x2={width} y2={laneTop} key="separator" />);
    }

    // Every series fills under its line, faintly, so where accounts overlap the wash adds up
    // but stays light; reset labels only where one series resets in the lane, and the
    // marker's title names it.
    const single = listed.series.length === 1;
    // Only a series the lane lists can be emphasized: one folded or searched away since
    // the pointer came to its row leaves the lane as it is.
    const emphasized = listed.series.some((series) => seriesKey(metric, series) === view.emphasizedSeries) ? view.emphasizedSeries : null;
    listed.series.forEach((series, seriesIndex) => {
      const key = seriesKey(metric, series);
      const emphasis = !emphasized ? "" : emphasized === key ? " is-emphasized" : " is-receded";
      const reading = laneReading(metric, series, range);
      if (reading.at === null) return;
      const markerAt = readingMarkerAt(metric, { at: reading.at, end: reading.end! }, range);
      const { resetTimes, segments } = metricSampleTimes(metric, series, range, width, markerAt);
      if (!segments.length) return;
      const providerClass = `is-${seriesTone(series)}${emphasis}`;
      // The line and its area stop at the edges of an empty bin and resume after it.
      const parts: ReactNode[] = segments.flatMap((instants, segmentIndex) => {
        const points = instants.map((instant) => ({ x: x(instant), y: y(metricValueAt(metric, series, instant)!) }));
        const linePath = points.map((point, pointIndex) => `${pointIndex === 0 ? "M" : "L"} ${point.x.toFixed(1)} ${point.y.toFixed(1)}`).join(" ");
        const areaPath = `${linePath} L ${points.at(-1)!.x.toFixed(1)} ${chartBottom} L ${points[0].x.toFixed(1)} ${chartBottom} Z`;
        return [
          <path className={`feld-agents__metric-area ${providerClass}`} d={areaPath} key={`area:${segmentIndex}`} />,
          <path className={`feld-agents__metric-line ${providerClass}`} d={linePath} data-metric-series={key} key={`line:${segmentIndex}`} />,
        ];
      });

      if (series.showResets) {
        resetTimes.forEach((reset) => {
          const resetX = x(reset);
          parts.push(<line className={`feld-agents__metric-reset${emphasis}`} data-metric-series={key} x1={resetX.toFixed(1)} y1={chartTop} x2={resetX.toFixed(1)} y2={chartBottom} key={`reset:${reset}`}><title>{model.resetTitle(metric, series, reset)}</title></line>);
          if (single) parts.push(<text className="feld-agents__metric-reset-label" x={(resetX + 4).toFixed(1)} y={chartTop + 10} key={`reset-label:${reset}`}>reset</text>);
        });
      }

      // The marker sits on the sample the reading names, so it never reads a value where
      // the line is at 0.
      parts.push(<circle className={`feld-agents__metric-point ${providerClass}`} data-metric-series={key} cx={x(markerAt).toFixed(1)} cy={y(metricValueAt(metric, series, markerAt)!).toFixed(1)} r="3" key="point"><title>{metricReadingTitle(metric, series, reading.at, reading.text)}</title></circle>);
      lane.push(<Fragment key={`series:${seriesIndex}`}>{parts}</Fragment>);
    });
    marks.push(<Fragment key={metric.id}>{lane}</Fragment>);
  });

  // A wider window keeps the Timeline's range lit and veils the rest, over the lines, so
  // a value still lines up with the Actors above it by time; the Metrics axis names the
  // wider scale and marks the Timeline's range.
  if (model.hasMetricsContext(view)) {
    const focus = model.rangeFor(view);
    const focusLeft = x(focus.start);
    const focusRight = x(focus.end);
    marks.push(
      <rect className="feld-agents__metric-context" x="0" y="0" width={Math.max(0, focusLeft).toFixed(1)} height={height} key="context:before" />,
      <rect className="feld-agents__metric-context" x={focusRight.toFixed(1)} y="0" width={Math.max(0, width - focusRight).toFixed(1)} height={height} key="context:after" />,
      ...[focusLeft, focusRight].map((edge, index) => <line className="feld-agents__metric-focus-edge" x1={edge.toFixed(1)} y1="0" x2={edge.toFixed(1)} y2={height} key={`focus-edge:${index}`} />),
    );
  }

  return { width, height, range, lanes, marks };
}

// The lanes' plot; its parent `.feld-agents__metrics-chart` is the page's.
export const MetricsSvg = memo(function MetricsSvg() {
  const model = useActorsModel();
  const view = useActorsStore(useShallow(selectMetricsView));
  const svgRef = useRef<SVGSVGElement>(null);
  const box = useParentBox(svgRef);
  const width = Math.max(box.minWidth, box.clientWidth || box.minWidth);
  const compact = useCompactLayout();
  const layers = useMemo(() => metricsLayers(model, view, width, compact), [model, view, width, compact]);
  const { range, height } = layers;
  const plotRight = width - PLOT_PAD_RIGHT;
  // The lane under the pointer marks the hovered instant with a dashed line and a point on
  // each series it draws there, and the tooltip reads them.
  const [hovered, setHovered] = useState<{ lane: ListedMetricLane; instant: Instant } | null>(null);
  const pointer = useRef<PointerAt | null>(null);
  const hover = useCallback((at: PointerAt | null) => {
    pointer.current = at;
    const svg = svgRef.current;
    const point = at && svg ? viewPointFor(svg, at.clientX, at.clientY) : null;
    const lane = point ? layers.lanes.find((candidate) => point.y >= candidate.top && point.y < candidate.top + candidate.height) : undefined;
    if (!point || !lane || point.x < PLOT_PAD_LEFT || point.x > plotRight) {
      setHovered(null);
      useChartTooltip.getState().hide();
      return;
    }
    const fraction = (point.x - PLOT_PAD_LEFT) / (plotRight - PLOT_PAD_LEFT);
    const instant = range.start + fraction * (range.end - range.start);
    setHovered({ lane, instant });
    useChartTooltip.getState().show(metricTooltip(model, lane, instant), at!.clientX, at!.clientY);
  }, [model, layers, range, plotRight]);
  // A new drawing reads the values now under a pointer that stayed.
  useLayoutEffect(() => {
    if (pointer.current) hover(pointer.current);
  }, [hover]);
  const handlers = usePlotHandlers(model, svgRef, "metrics", hover);
  let markers: ReactNode = null;
  if (hovered) {
    const { lane: { metric, series, top, height: laneHeight }, instant } = hovered;
    const chartTop = top + METRIC_CHART_INSET.top;
    const chartHeight = laneHeight - METRIC_CHART_INSET.top - METRIC_CHART_INSET.bottom;
    const hoverX = plotX(range, width, instant).toFixed(1);
    markers = (
      <>
        <line className="feld-agents__metric-hover" x1={hoverX} y1={chartTop} x2={hoverX} y2={chartTop + chartHeight} />
        {series.filter((item) => seriesSampledAt(metric, item, instant)).map((item, index) => (
          <circle className={`feld-agents__metric-hover feld-agents__metric-point is-${seriesTone(item)}`} cx={hoverX} cy={(chartTop + chartHeight - metricValueAt(metric, item, instant)! / metric.maximum * chartHeight).toFixed(1)} r="3.5" key={index} />
        ))}
      </>
    );
  }
  return (
    <svg role="img" aria-label={`Provider and runtime metrics in ${model.formatRange(range)}`} data-metrics-timeline="" viewBox={`0 0 ${width} ${height}`} height={height} ref={svgRef} {...handlers}>
      {layers.marks}
      {markers}
    </svg>
  );
});
