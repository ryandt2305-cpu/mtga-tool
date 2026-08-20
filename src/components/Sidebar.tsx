// Navigation rail — 56px icon sidebar with logo, nav items, settings, status dot.
// Controls navigation via shared MemoryHistory instance.

import { createSignal, For, type Component } from "solid-js";
import type { MemoryHistory } from "@solidjs/router";
import NavButton from "./NavButton";
import StatusIndicator from "./StatusIndicator";
import { IconCollection, IconDecks, IconEconomy, IconEvents, IconFeed, IconSettings } from "./icons";
import { connectionLevel } from "../stores/connectionStore";

interface SidebarProps {
  history: MemoryHistory;
}

const navItems = [
  { path: "/", label: "Collection", icon: IconCollection },
  { path: "/economy", label: "Economy", icon: IconEconomy },
  { path: "/events", label: "Events", icon: IconEvents },
  { path: "/feed", label: "Feed", icon: IconFeed },
  { path: "/decks", label: "Decks", icon: IconDecks },
] as const;

const Sidebar: Component<SidebarProps> = (props) => {
  const [currentPath, setCurrentPath] = createSignal("/");

  const navigate = (path: string) => {
    setCurrentPath(path);
    props.history.set({ value: path });
  };

  return (
    <aside class="sidebar">
      <div class="sidebar-logo">
        <img src="/favicon.png" alt="MTGA Hub" width="28" height="28" />
      </div>

      <nav class="sidebar-nav">
        <For each={navItems}>
          {(item) => (
            <NavButton
              icon={item.icon}
              label={item.label}
              active={currentPath() === item.path}
              onClick={() => navigate(item.path)}
            />
          )}
        </For>
      </nav>

      <div class="sidebar-spacer" />

      <NavButton
        icon={IconSettings}
        label="Settings"
        active={currentPath() === "/settings"}
        onClick={() => navigate("/settings")}
      />

      <div class="sidebar-status">
        <StatusIndicator level={connectionLevel()} variant="dot" />
      </div>
    </aside>
  );
};

export default Sidebar;
