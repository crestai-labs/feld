// The toasts at the page's corner: what a request from an Actions menu is doing. One toast
// per request, kept by its id: it starts as pending, and the daemon's answer settles it
// in place as done or refused. A pending toast stays until settled; a settled one leaves
// on its own after a while, or when dismissed.
import { create } from "zustand";

export type ToastKind = "pending" | "done" | "error";

export interface Toast {
  id: string;
  kind: ToastKind;
  text: string;
  detail: string | null;
}

interface ToastStore {
  toasts: Toast[];
  show(toast: Toast): void;
  dismiss(id: string): void;
}

// How long a settled toast stays, in milliseconds; a pending one has no limit.
export const TOAST_LINGER: Readonly<Record<ToastKind, number | null>> = Object.freeze({ pending: null, done: 6000, error: 15000 });

const timers = new Map<string, ReturnType<typeof setTimeout>>();

function forgetTimer(id: string) {
  const held = timers.get(id);
  if (held === undefined) return;
  clearTimeout(held);
  timers.delete(id);
}

export const useToasts = create<ToastStore>((set, get) => ({
  toasts: [],
  show(toast) {
    forgetTimer(toast.id);
    set((state) => {
      const held = state.toasts.findIndex((entry) => entry.id === toast.id);
      const toasts = held === -1 ? [...state.toasts, toast] : state.toasts.with(held, toast);
      return { toasts };
    });
    const linger = TOAST_LINGER[toast.kind];
    if (linger !== null) timers.set(toast.id, setTimeout(() => get().dismiss(toast.id), linger));
  },
  dismiss(id) {
    forgetTimer(id);
    set((state) => (state.toasts.some((entry) => entry.id === id) ? { toasts: state.toasts.filter((entry) => entry.id !== id) } : state));
  },
}));

export function clearToasts() {
  for (const id of [...timers.keys()]) forgetTimer(id);
  useToasts.setState({ toasts: [] });
}
