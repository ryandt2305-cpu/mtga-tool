// Events dashboard — active event courses (drafts, sealed, constructed events).
// Data comes from LogService's CourseList parsing via eventsStore.

import { type Component, Show, For } from "solid-js";
import Header from "../components/Header";
import { eventCourses, hasEvents } from "../stores/eventsStore";
import { parseEventName } from "../stores/feedBuildersEvent";
import type { EventCourse } from "../lib/tauri";

/** Map CurrentModule to a display label and CSS class. */
function moduleState(module: string): { label: string; cssClass: string } {
  const map: Record<string, { label: string; cssClass: string }> = {
    Playing: { label: "Playing", cssClass: "event-module-playing" },
    DeckSubmit: { label: "Building Deck", cssClass: "event-module-building" },
    DraftPick: { label: "Drafting", cssClass: "event-module-building" },
    ReadyToComplete: { label: "Complete", cssClass: "event-module-complete" },
    WaitingForMatch: { label: "Waiting", cssClass: "event-module-waiting" },
    Completed: { label: "Complete", cssClass: "event-module-complete" },
    Pending: { label: "Pending", cssClass: "event-module-waiting" },
  };
  return map[module] ?? { label: module || "Unknown", cssClass: "event-module-default" };
}

/** Format win/loss record: "3-1" or "—" if no data. */
function formatRecord(course: EventCourse): string {
  if (course.wins == null && course.losses == null) return "\u2014";
  return `${course.wins ?? 0}-${course.losses ?? 0}`;
}

// --- Component ---

const Events: Component = () => {
  return (
    <div class="view">
      <Header title="Events" />
      <div class="view-content">
        <Show when={hasEvents()} fallback={<EmptyState />}>
          <section class="events-section">
            <h2 class="economy-section-title">Active Events</h2>
            <div class="events-list">
              <For each={eventCourses()}>
                {(course) => <EventCard course={course} />}
              </For>
            </div>
          </section>
        </Show>
      </div>
    </div>
  );
};

// --- Sub-components ---

const EmptyState: Component = () => (
  <div class="scan-prompt">
    <p class="scan-prompt-text">
      No active events. Join a draft, sealed, or constructed event in MTGA to
      see it here.
    </p>
  </div>
);

const EventCard: Component<{ course: EventCourse }> = (props) => {
  const name = () => parseEventName(props.course.internal_event_name);
  const state = () => moduleState(props.course.current_module);
  const record = () => formatRecord(props.course);
  const hasProgress = () =>
    props.course.wins != null && props.course.max_wins != null && props.course.max_wins > 0;
  const progressPct = () => {
    if (!hasProgress()) return 0;
    return Math.min(100, ((props.course.wins ?? 0) / props.course.max_wins!) * 100);
  };

  return (
    <div class="event-card">
      <div class="event-card-header">
        <span class="event-name">{name()}</span>
        <span class={`event-module ${state().cssClass}`}>{state().label}</span>
      </div>
      <div class="event-card-body">
        <div class="event-record">
          <span class="event-record-label">Record</span>
          <span class="event-record-value">{record()}</span>
          <Show when={props.course.max_wins != null}>
            <span class="event-record-limit">
              (max {props.course.max_wins}W / {props.course.max_losses ?? "?"}L)
            </span>
          </Show>
        </div>
        <Show when={hasProgress()}>
          <div class="event-progress-bar">
            <div class="event-progress-fill" style={{ width: `${progressPct()}%` }} />
          </div>
        </Show>
      </div>
    </div>
  );
};

export default Events;
