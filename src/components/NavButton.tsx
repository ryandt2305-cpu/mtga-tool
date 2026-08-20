// Reusable nav button for sidebar. Renders icon + handles active/hover state.

import type { Component } from "solid-js";

interface NavButtonProps {
  icon: Component;
  label: string;
  active: boolean;
  onClick: () => void;
}

const NavButton: Component<NavButtonProps> = (props) => {
  return (
    <button
      class={`nav-btn${props.active ? " active" : ""}`}
      title={props.label}
      onClick={props.onClick}
    >
      {props.icon({})}
    </button>
  );
};

export default NavButton;
