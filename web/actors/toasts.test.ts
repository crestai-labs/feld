import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { TOAST_LINGER, clearToasts, useToasts } from "./toasts.ts";

beforeEach(() => {
  vi.useFakeTimers();
  clearToasts();
});

afterEach(() => {
  clearToasts();
  vi.useRealTimers();
});

const toasts = () => useToasts.getState().toasts;
const show = useToasts.getState().show;

test("a pending toast stays until its request settles it in place, and a settled one leaves on its own", () => {
  show({ id: "a", kind: "pending", text: "Pausing the Schedule · nightly", detail: null });
  vi.advanceTimersByTime(60_000);
  expect(toasts().map((toast) => toast.kind)).toEqual(["pending"]);

  show({ id: "b", kind: "pending", text: "Stop · dev", detail: null });
  show({ id: "a", kind: "done", text: "Schedule paused · nightly", detail: "No tick fires until resumed." });
  // Settled in place: the first toast keeps its position.
  expect(toasts().map((toast) => `${toast.id}:${toast.kind}`)).toEqual(["a:done", "b:pending"]);
  vi.advanceTimersByTime(TOAST_LINGER.done! - 1);
  expect(toasts()).toHaveLength(2);
  vi.advanceTimersByTime(1);
  expect(toasts().map((toast) => toast.id)).toEqual(["b"]);
});

test("a refused request's toast stays longer, and any toast can be dismissed", () => {
  show({ id: "a", kind: "error", text: "Schedule not paused · nightly", detail: "no Schedule" });
  vi.advanceTimersByTime(TOAST_LINGER.done!);
  expect(toasts()).toHaveLength(1);
  useToasts.getState().dismiss("a");
  expect(toasts()).toEqual([]);
  // A dismissed toast's timer is gone with it.
  show({ id: "a", kind: "done", text: "again", detail: null });
  vi.advanceTimersByTime(TOAST_LINGER.error!);
  expect(toasts()).toEqual([]);
});
