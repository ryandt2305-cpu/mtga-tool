// Reusable nav button for sidebar. Renders icon + handles active/hover state.

import type { Component } from "solid-js";
import Tooltip from "./Tooltip";

interface NavButtonProps {
  icon: Component;
  label: string;
  active: boolean;
  onClick: () => void;
}

const NavButton: Component<NavButtonProps> = (props) => {
  return (
    <Tooltip text={props.label} placement="right">
      <button
        class={`nav-btn${props.active ? " active" : ""}`}
        onClick={props.onClick}
      >
        {props.icon({})}
      </button>
    </Tooltip>
  );
};

export default NavButton;
