import { createContext, useContext } from "react";

// The compact shell's drawer, given to the page only on the compact layout: the place in
// the drawer where a page lays its own list (the Actors page's Workspaces), whether the
// drawer is open, and opening and closing it. Closing returns the focus to the control
// that opened it.
export type Drawer = {
  slot: HTMLElement | null;
  expanded: boolean;
  open(): void;
  close(): void;
};

export const DRAWER_ID = "shell-drawer";

export const DrawerContext = createContext<Drawer | null>(null);

export function useDrawer() {
  return useContext(DrawerContext);
}
