// ManaSymbol — bundled Scryfall mana symbol (WUBRG, plus colourless).
// Files live under public/icons/mana/{letter}.svg.

import type { Component } from "solid-js";
import Tooltip from "./Tooltip";

export type ManaLetter = "W" | "U" | "B" | "R" | "G" | "C";

interface Props {
  letter: ManaLetter;
  size?: number;
  title?: string;
}

const ManaSymbol: Component<Props> = (props) => {
  const size = () => props.size ?? 14;
  return (
    <Tooltip text={props.title ?? props.letter}>
      <img
        class="mana-symbol"
        src={`/icons/mana/${props.letter}.svg`}
        alt={props.letter}
        width={size()}
        height={size()}
        style={{ width: `${size()}px`, height: `${size()}px` }}
      />
    </Tooltip>
  );
};

export default ManaSymbol;
