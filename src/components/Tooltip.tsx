// Tooltip — hover-triggered text label that replaces native `title=""`.
// Uses FloatingLayer so it portals to <body>, sits above modals, and clamps
// to the viewport. 250ms open delay to avoid flashing on quick mouse-through.

import {
  type Component,
  type JSX,
  Show,
  createSignal,
  onCleanup,
} from "solid-js";
import FloatingLayer, { type Placement } from "./FloatingLayer";

interface Props {
  text: string | null | undefined;
  placement?: Placement;
  delay?: number;
  children: JSX.Element;
  /** Extra class on the anchor span. */
  class?: string;
  /** Inline style on the anchor span. */
  style?: JSX.CSSProperties | string;
  /** When true, wrapper uses display:contents so nesting stays valid inside
   *  ul/table/etc. The child element becomes the position anchor. */
  contents?: boolean;
}

const Tooltip: Component<Props> = (props) => {
  const [anchor, setAnchor] = createSignal<HTMLElement | null>(null);
  const [open, setOpen] = createSignal(false);
  let openTimer: ReturnType<typeof setTimeout> | null = null;

  function clearTimer(): void {
    if (openTimer !== null) { clearTimeout(openTimer); openTimer = null; }
  }
  function schedule(el: HTMLElement): void {
    clearTimer();
    const d = props.delay ?? 250;
    if (d <= 0) { setAnchor(el); setOpen(true); return; }
    openTimer = setTimeout(() => {
      openTimer = null;
      setAnchor(el);
      setOpen(true);
    }, d);
  }
  function dismiss(): void {
    clearTimer();
    setOpen(false);
    setAnchor(null);
  }
  onCleanup(clearTimer);

  const hasText = () => typeof props.text === "string" && props.text.length > 0;

  function pickAnchor(host: HTMLElement): HTMLElement {
    // In `contents` mode the span is display:contents and returns a zero-rect;
    // use the first element child instead.
    if (props.contents) {
      const child = host.firstElementChild as HTMLElement | null;
      return child ?? host;
    }
    return host;
  }

  return (
    <>
      <span
        class={`tooltip-anchor ${props.contents ? "tooltip-anchor--contents" : ""} ${props.class ?? ""}`}
        style={props.style}
        onMouseEnter={(e) => hasText() && schedule(pickAnchor(e.currentTarget as HTMLElement))}
        onMouseLeave={dismiss}
        onFocusIn={(e) => hasText() && schedule(pickAnchor(e.currentTarget as HTMLElement))}
        onFocusOut={dismiss}
        onClick={dismiss}
      >
        {props.children}
      </span>
      <Show when={open() && hasText() && anchor() !== null}>
        <FloatingLayer
          anchor={anchor()}
          placement={props.placement ?? "top"}
          gap={6}
          zIndex={3000}
        >
          <div class="tooltip-bubble">{props.text}</div>
        </FloatingLayer>
      </Show>
    </>
  );
};

export default Tooltip;
