// SavedDecks — list of persisted decks with Load and Delete.
// Load sets result() from result_json; Delete removes and refreshes the list.

import { type Component, For, Show, onMount } from "solid-js";
import { savedDecks, setStep } from "../../stores/deckStore";
import {
  deleteSaved,
  loadSaved,
  refreshSavedDecks,
} from "../../stores/deckStoreActions";

function formatUpdated(ts: number): string {
  const d = new Date(ts * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleString();
}

const SavedDecks: Component = () => {
  onMount(() => {
    void refreshSavedDecks();
  });

  return (
    <div class="decks-saved-wrap">
      <Show
        when={savedDecks().length > 0}
        fallback={<div class="decks-empty">No saved decks yet.</div>}
      >
        <ul class="decks-saved-list decks-saved-list--full">
          <For each={savedDecks()}>
            {(d) => (
              <li class="decks-saved-row decks-saved-row--full">
                <div class="decks-saved-main">
                  <span class="decks-saved-name">{d.name}</span>
                  <span class="decks-saved-meta">
                    {d.format} · updated {formatUpdated(d.updated_at)}
                  </span>
                </div>
                <div class="decks-saved-actions">
                  <button
                    class="decks-saved-btn"
                    onClick={async () => {
                      await loadSaved(d.id);
                      setStep("deck");
                    }}
                  >
                    Load
                  </button>
                  <button
                    class="decks-saved-btn decks-saved-btn--danger"
                    onClick={() => void deleteSaved(d.id)}
                  >
                    Delete
                  </button>
                </div>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </div>
  );
};

export default SavedDecks;
