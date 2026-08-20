// FloatingLayer — shared primitive for anything that floats above the page
// (tooltips, hover previews, popovers). Portals to <body> so parent overflow
// and z-index can't clip it, positions relative to an anchor rect, and clamps
// to the viewport (flipping to the opposite side if the preferred side would
// overflow). Recomputes on scroll/resize while visible.

import {
  type Component,
  type JSX,
  createEffect,
  createSignal,
  onCleanup,
} from "solid-js";
import { Portal } from "solid-js/web";

export type Placement = "top" | "bottom" | "left" | "right";

interface Props {
  /** Anchor element to position against; if null, the layer hides. */
  anchor: HTMLElement | null;
  /** Preferred side. Flips to the opposite if it would overflow. Default: bottom. */
  placement?: Placement;
  /** Gap in px between anchor and layer. Default 8. */
  gap?: number;
  /** Viewport padding to keep clear. Default 8. */
  padding?: number;
  /** z-index. Default 2000. */
  zIndex?: number;
  /** Extra class on the outer portaled element. */
  class?: string;
  children: JSX.Element;
}

interface Rect { x: number; y: number }

const FloatingLayer: Component<Props> = (props) => {
  const [pos, setPos] = createSignal<Rect | null>(null);
  let layerEl: HTMLDivElement | undefined;

  function measure(): void {
    const anchor = props.anchor;
    const layer = layerEl;
    if (!anchor || !layer) { setPos(null); return; }
    const gap = props.gap ?? 8;
    const pad = props.padding ?? 8;
    const placement: Placement = props.placement ?? "bottom";
    const a = anchor.getBoundingClientRect();
    const w = layer.offsetWidth;
    const h = layer.offsetHeight;
    if (w === 0 || h === 0) return;
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    function fits(p: Placement): boolean {
      switch (p) {
        case "top":    return a.top - gap - h >= pad;
        case "bottom": return a.bottom + gap + h <= vh - pad;
        case "left":   return a.left - gap - w >= pad;
        case "right":  return a.right + gap + w <= vw - pad;
      }
    }
    const opposite: Record<Placement, Placement> = {
      top: "bottom", bottom: "top", left: "right", right: "left",
    };
    let side: Placement = placement;
    if (!fits(side)) {
      const alt = opposite[side];
      if (fits(alt)) side = alt;
      // else fall through — pick original and clamp.
    }

    let x = 0;
    let y = 0;
    if (side === "top" || side === "bottom") {
      // Center over the anchor horizontally by default.
      x = a.left + (a.width - w) / 2;
      y = side === "top" ? a.top - gap - h : a.bottom + gap;
    } else {
      x = side === "left" ? a.left - gap - w : a.right + gap;
      y = a.top + (a.height - h) / 2;
    }
    if (x + w > vw - pad) x = vw - pad - w;
    if (x < pad) x = pad;
    if (y + h > vh - pad) y = vh - pad - h;
    if (y < pad) y = pad;
    setPos({ x, y });
  }

  createEffect(() => {
    // Re-run whenever the anchor changes or content re-renders.
    props.anchor;
    // Give the browser a paint pass so offsetWidth/Height are populated.
    if (props.anchor) requestAnimationFrame(measure);
    else setPos(null);
  });

  function onWindow(): void { measure(); }
  window.addEventListener("scroll", onWindow, true);
  window.addEventListener("resize", onWindow);
  onCleanup(() => {
    window.removeEventListener("scroll", onWindow, true);
    window.removeEventListener("resize", onWindow);
  });

  return (
    <Portal>
      <div
        ref={layerEl}
        class={`floating-layer ${props.class ?? ""}`}
        style={{
          position: "fixed",
          "z-index": String(props.zIndex ?? 2000),
          left: `${pos()?.x ?? -9999}px`,
          top: `${pos()?.y ?? -9999}px`,
          visibility: pos() === null ? "hidden" : "visible",
        }}
      >
        {props.children}
      </div>
    </Portal>
  );
};

export default FloatingLayer;
