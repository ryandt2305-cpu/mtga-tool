// Shared status component — dot variant for sidebar, badge variant for settings.

import type { Component } from "solid-js";
import type { ConnectionLevel } from "../stores/connectionStore";

interface StatusIndicatorProps {
  level: ConnectionLevel;
  variant: "dot" | "badge";
  label?: string;
}

function stateClass(level: ConnectionLevel): string {
  switch (level) {
    case "live": return "connected";
    case "offline": return "offline";
    case "disconnected": return "disconnected";
  }
}

const StatusIndicator: Component<StatusIndicatorProps> = (props) => {
  const cls = () => stateClass(props.level);

  return props.variant === "dot" ? (
    <span class={`status-dot ${cls()}`} />
  ) : (
    <span class={`status-badge ${cls()}`}>
      <span class={`status-badge-dot ${cls()}`} />
      {props.label}
    </span>
  );
};

export default StatusIndicator;
