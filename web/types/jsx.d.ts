import type { DetailedHTMLProps, HTMLAttributes } from "react";

// The pages draw in host elements, which the stylesheets select by tag name.
type HostElement = DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement>;

declare module "react" {
  namespace JSX {
    interface IntrinsicElements {
      "feld-agents": HostElement;
      "feld-pairing": HostElement;
      "feld-settings": HostElement;
    }
  }
}
