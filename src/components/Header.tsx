// Contextual header bar. Takes title + optional children slot for actions.

import { Show, type Component, type JSX } from "solid-js";
import { offlineAgeLabel } from "../stores/connectionStore";

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
        <span class="header-stale-badge" title="MTGA is not running — showing last-synced data">
          {(() => {
            const age = offlineAgeLabel();
            return age ? `· offline (${age})` : "· offline";
          })()}
        </span>
      </Show>
      {props.children}
    </header>
  );
};

export default Header;
