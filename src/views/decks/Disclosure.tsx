// Disclosure — collapsible row primitive shared by the StatsRail sections
// (extracted from StatsRail.tsx, which is over the 500-line soft limit).

import { type Component, Show, createSignal, type JSX } from "solid-js";

const Disclosure: Component<{
  label: string;
  summary: () => JSX.Element;
  tone?: "default" | "warn" | "error";
  defaultOpen?: boolean;
  children: JSX.Element;
}> = (props) => {
  const [open, setOpen] = createSignal(props.defaultOpen ?? false);
  const tone = () => props.tone ?? "default";
  return (
    <div class={`decks-disclosure-row decks-disclosure-row--${tone()}`}>
      <button class="decks-disclosure-head" onClick={() => setOpen(!open())}>
        <span class="decks-disclosure-caret">{open() ? "▾" : "▸"}</span>
        <span class="decks-disclosure-label">{props.label}</span>
        <span class="decks-disclosure-summary">{props.summary()}</span>
      </button>
      <Show when={open()}>
        <div class="decks-disclosure-body">{props.children}</div>
      </Show>
    </div>
  );
};

export default Disclosure;
