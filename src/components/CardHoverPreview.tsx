// CardHoverPreview — wraps children in a span that reveals a floating card
// image while the pointer is over it. Resolves the card by name via the
// collection index; renders nothing if the name is unknown. Positioning goes
// through FloatingLayer so the preview stays on-screen and above modals.

import {
  type Component,
  type JSX,
  Show,
  createMemo,
  createSignal,
} from "solid-js";
import { cardsByName } from "../stores/collectionStore";
import { getCardImageUrl } from "../stores/priceStore";
import type { CardInfo } from "../lib/tauri";
import FloatingLayer from "./FloatingLayer";

interface Props {
  name: string;
  children: JSX.Element;
  class?: string;
}

const CardHoverPreview: Component<Props> = (props) => {
  const [anchor, setAnchor] = createSignal<HTMLElement | null>(null);

  const card = createMemo<CardInfo | undefined>(() => {
    const index = cardsByName();
    const name = props.name;
    let printings = index.get(name);
    if (!printings) {
      const lower = name.toLowerCase();
      for (const [key, list] of index) {
        if (key.toLowerCase() === lower) { printings = list; break; }
      }
    }
    return printings && printings.length > 0 ? printings[0] : undefined;
  });

  return (
    <>
      <span
        class={`card-hover-anchor ${props.class ?? ""}`}
        onMouseEnter={(e) => setAnchor(e.currentTarget as HTMLElement)}
        onMouseLeave={() => setAnchor(null)}
      >
        {props.children}
      </span>
      <Show when={anchor() !== null && card() !== undefined}>
        <FloatingLayer anchor={anchor()} placement="right" gap={12} zIndex={2500}>
          <div class="card-hover-preview">
            <img src={getCardImageUrl(card()!, "normal")} alt={card()!.name} />
          </div>
        </FloatingLayer>
      </Show>
    </>
  );
};

export default CardHoverPreview;
