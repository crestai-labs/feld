// A time axis row under the Timeline or the Metrics lanes: an empty cell under the label
// columns, the axis under the plot, and an empty cell under Actions, each pinned like the
// cells above it. The labels are one line (see axisLabels). The Metrics axis of a window
// wider than the Timeline's marks the Timeline's range (`focus`) above its ticks.

import { memo, useRef } from "react";
import { useActorsModel } from "../context.ts";
import { AXIS_ROW_HEIGHT, PLOT_PAD_LEFT, PLOT_PAD_RIGHT, axisLabels, axisStep, axisTicks, isMidnight, minorStepOf, stepTimes } from "../model.ts";
import { useParentBox } from "./interaction.ts";

interface AxisProps {
  range: FeldMetricRange;
  attribute: "data-timeline-axis" | "data-metrics-axis";
  focus?: FeldMetricRange | null;
}

// The width of a part of a label in the axis font; the date part is bold.
const AXIS_FONT = '10.5px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
let measureContext: CanvasRenderingContext2D | null | undefined;
function measureLabel(text: string, bold: boolean) {
  if (measureContext === undefined) measureContext = document.createElement("canvas").getContext("2d");
  if (!measureContext) return text.length * 6.5;
  measureContext.font = bold ? `600 ${AXIS_FONT}` : AXIS_FONT;
  return measureContext.measureText(text).width;
}

const TimeAxis = memo(function TimeAxis({ range, attribute, focus = null }: AxisProps) {
  const model = useActorsModel();
  const svgRef = useRef<SVGSVGElement>(null);
  const box = useParentBox(svgRef);
  const width = Math.max(1, box.clientWidth);
  const zone = model.TIME_ZONE;
  const usable = width - PLOT_PAD_LEFT - PLOT_PAD_RIGHT;
  const x = (instant: number) => PLOT_PAD_LEFT + (instant - range.start) / (range.end - range.start) * usable;
  const minorStep = usable > 0 ? minorStepOf(axisStep(range, usable)) : null;
  const ticks = usable > 0 ? axisTicks(range, usable, zone) : [];
  const labels = axisLabels(ticks, x, width, { date: (instant) => model.shortDate.format(instant), time: (instant) => model.timeOnly.format(instant) }, measureLabel);
  return (
    <svg viewBox={`0 0 ${width} ${AXIS_ROW_HEIGHT}`} height={AXIS_ROW_HEIGHT} ref={svgRef} {...{ [attribute]: "" }}>
      {focus ? (
        <rect className="feld-agents__metric-focus-bar" x={x(focus.start).toFixed(1)} y="1" width={Math.max(2, x(focus.end) - x(focus.start)).toFixed(1)} height="4" rx="2">
          <title>{`Timeline range ${model.formatRange(focus)}`}</title>
        </rect>
      ) : null}
      {minorStep ? stepTimes(range, minorStep, zone).map((instant) => (
        <line className="feld-agents__axis-tick is-minor" x1={x(instant).toFixed(1)} y1="0" x2={x(instant).toFixed(1)} y2="3" key={`minor:${instant}`} />
      )) : null}
      {ticks.map((instant) => {
        const tickX = x(instant).toFixed(1);
        return <line className={`feld-agents__axis-tick${isMidnight(instant, zone) ? " is-day" : ""}`} x1={tickX} y1="0" x2={tickX} y2="4" key={instant} />;
      })}
      {labels.map(({ instant, left, date, time }) => (
        <text className="feld-agents__axis-label" x={left.toFixed(1)} y="16" textAnchor="start" key={`label:${instant}`}>
          {date ? <tspan className="feld-agents__axis-date">{date}</tspan> : null}
          {date && time ? " " : null}
          {time}
        </text>
      ))}
    </svg>
  );
});

export function AxisRow({ range, attribute, focus = null }: AxisProps) {
  return (
    <div className="feld-agents__axis-row" aria-hidden="true">
      <div className="feld-agents__axis-lead"></div>
      <div className="feld-agents__axis"><TimeAxis range={range} attribute={attribute} focus={focus} /></div>
      <div className="feld-agents__axis-tail"></div>
    </div>
  );
}
