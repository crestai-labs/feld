// The fixture's observation instant and the timezone the mock shows instants in. Every
// fixture and component reads time from here, so the pages look the same whenever they
// are opened.
(function registerFixtureClock(global) {
  "use strict";
  const ui = global.FeldWebUI = global.FeldWebUI || {};
  ui.data = ui.data || {};
  ui.data.clock = Object.freeze({
    now: Date.parse("2026-09-02T15:00:00+09:00"),
    timeZone: "Asia/Tokyo",
  });
})(window);
