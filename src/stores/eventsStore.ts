// Events state. Tracks active event courses (drafts, sealed, constructed events).
// Subscribes to log:events_updated and hydrates from backend on startup.

import { createSignal, createMemo, createRoot } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  Events,
  subscribe,
  getEventCourses,
  type EventCourse,
  type EventsUpdatedPayload,
  type StartupCompletePayload,
} from "../lib/tauri";

// --- Signals ---

const [eventCourses, setEventCourses] = createSignal<EventCourse[]>([]);
// Module-scope memo needs a stable root owner. Store lives for the app
// lifetime, so a root is fine.
const hasEvents = createRoot(() => createMemo(() => eventCourses().length > 0));

export { eventCourses, hasEvents };

// --- Internal ---

async function refreshCourses(): Promise<void> {
  try {
    const courses = await getEventCourses();
    setEventCourses(courses);
  } catch {
    // Backend not ready — will retry on next event
  }
}

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initEventsStore(): Promise<void> {
  unlisteners.push(
    await subscribe<StartupCompletePayload>(
      Events.app.STARTUP_COMPLETE,
      () => refreshCourses()
    ),

    await subscribe<EventsUpdatedPayload>(
      Events.log.EVENTS_UPDATED,
      (p) => setEventCourses(p.courses)
    )
  );

  // Hydrate from current backend state
  await refreshCourses();
}

export function cleanupEventsStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
}
