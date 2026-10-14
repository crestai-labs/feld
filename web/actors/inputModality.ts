// Whether the last input came from a pointer or the keyboard. A focus the page moves
// itself (back to a Session's line when its Console closes, to a lane when its Actor's
// row is clicked) is drawn with its outline for the keyboard alone: after a pointer, the
// element is marked `data-pointer-focus`, which its focus styles read, until it blurs or a
// key moves the focus on.
import { useEffect } from "react";

const MARK = "data-pointer-focus";
let pointer = false;

export function lastInputWasPointer() {
  return pointer;
}

// Marks `element` when the last input was a pointer, ahead of focusing it.
export function markPointerFocus(element: Element) {
  if (pointer) element.setAttribute(MARK, "");
}

function onPointerDown() {
  pointer = true;
}

function onKeyDown() {
  pointer = false;
  document.activeElement?.removeAttribute(MARK);
}

function onFocusOut(event: FocusEvent) {
  (event.target as Element | null)?.removeAttribute?.(MARK);
}

export function useInputModality() {
  useEffect(() => {
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("focusout", onFocusOut, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("focusout", onFocusOut, true);
      pointer = false;
    };
  }, []);
}
