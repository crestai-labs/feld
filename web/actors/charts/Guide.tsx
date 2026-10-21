// ---- Timeline guide ----------------------------------------------------------------
// Every mark drawn in the Actors chart, with the shape as it is drawn and the rule it
// follows. The Timeline column's header opens it (on the compact layout, Timeline display
// does); the page draws no standing legend.

import { memo, useEffect, useRef } from "react";
import { TIMELINE_LEGEND } from "../model.ts";
import { useActorsStore } from "../store.ts";
import { timelineGuideOpener, usePageActions } from "../ui.tsx";
import { LegendGlyph } from "./glyphs.tsx";

// The guide is one of the page-level dialogs: a scrim that closes it (for the pointer only)
// and a titled panel.
export const TimelineGuide = memo(function TimelineGuide() {
  const open = useActorsStore((state) => state.timelineGuideOpen);
  const closeTimelineGuide = useActorsStore((state) => state.closeTimelineGuide);
  const { focus } = usePageActions();
  const closeRef = useRef<HTMLButtonElement>(null);

  // Opening the guide moves focus into it once.
  useEffect(() => {
    if (open) closeRef.current?.focus();
  }, [open]);

  if (!open) return null;
  // Closing returns focus to the control that opens the guide.
  const close = () => {
    closeTimelineGuide();
    focus(timelineGuideOpener());
  };
  return (
    <div className="feld-agents__display-modal">
      <button className="feld-agents__display-scrim" type="button" tabIndex={-1} aria-hidden="true" data-action="close-timeline-guide" onClick={close}></button>
      <div className="feld-agents__display-menu is-guide" role="dialog" aria-modal="true" aria-labelledby="feld-agents-timeline-guide-title">
        <div className="feld-agents__display-menu-title">
          <div>
            <strong id="feld-agents-timeline-guide-title">Timeline guide</strong>
            <span>One lane per Actor, time from left to right. Every mark is a record.</span>
          </div>
          <button className="feld-agents__display-menu-close" type="button" data-action="close-timeline-guide" aria-label="Close Timeline guide" ref={closeRef} onClick={close}>×</button>
        </div>
        {TIMELINE_LEGEND.map((group) => (
          <section className="feld-agents__guide-group" key={group.name}>
            <h3>{group.name}</h3>
            <ul className="feld-agents__guide-list">
              {group.entries.map((entry) => (
                <li key={entry.symbol}>
                  <LegendGlyph symbol={entry.symbol} />
                  <span><strong>{entry.label}</strong><small>{entry.detail}</small></span>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
});
