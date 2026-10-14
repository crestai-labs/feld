// The toasts at the page's corner (see toasts.ts).
import { useToasts } from "./toasts.ts";

export function Toasts() {
  const toasts = useToasts((state) => state.toasts);
  const dismiss = useToasts((state) => state.dismiss);
  if (!toasts.length) return null;
  return (
    <div className="feld-agents__toasts" role="status" aria-live="polite" data-toasts="">
      {toasts.map((toast) => (
        <div key={toast.id} className={`feld-agents__toast is-${toast.kind}`} data-toast={toast.kind}>
          <span className="feld-agents__toast-mark" aria-hidden="true">
            {toast.kind === "pending"
              ? <span className="feld-agents__toast-spinner"></span>
              : <svg viewBox="0 0 12 12">{toast.kind === "done" ? <path d="m2.5 6.3 2.4 2.4 4.6-5.2"></path> : <path d="m3 3 6 6M9 3 3 9"></path>}</svg>}
          </span>
          <span className="feld-agents__toast-text">
            <strong>{toast.text}</strong>
            {toast.detail ? <small>{toast.detail}</small> : null}
          </span>
          <button type="button" className="feld-agents__toast-close" aria-label="Dismiss" onClick={() => dismiss(toast.id)}>×</button>
        </div>
      ))}
    </div>
  );
}
