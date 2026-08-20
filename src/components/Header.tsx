// Contextual header bar. Takes title + optional children slot for actions.

import { Show, type Component, type JSX } from "solid-js";
import { offlineAgeLabel } from "../stores/connectionStore";
import Tooltip from "./Tooltip";

interface HeaderProps {
  title: string;
  children?: JSX.Element;
  staleBadge?: boolean;
}

const Header: Component<HeaderProps> = (props) => {
  return (
    <header class="header">
      <h1 class="header-title">{props.title}</h1>
      <Show when={props.staleBadge}>
        <Tooltip text="MTGA is not running — showing last-synced data">
          <span class="header-stale-badge">
            {(() => {
              const age = offlineAgeLabel();
              return age ? `· offline (${age})` : "· offline";
            })()}
          </span>
        </Tooltip>
      </Show>
      {props.children}
    </header>
  );
};

export default Header;
