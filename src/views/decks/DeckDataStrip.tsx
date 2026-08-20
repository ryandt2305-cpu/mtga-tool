// DeckDataStrip — single contextual strip below the Decks header. Replaces
// the old StatusPill + FirstRunBanner + separate Refresh button. Morphs
// across: first-run download, ingest-in-progress, error, idle-with-data,
// and initial-loading states.

import { type Component, Show, createMemo } from "solid-js";
import { ingest, status } from "../../stores/deckStore";
import { refreshStatus } from "../../stores/deckStoreActions";
import { deckRefreshBulk } from "../../lib/tauri";

function formatDate(iso: string | null): string {
  if (iso === null) return "never";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleDateString();
}

async function onRefresh(): Promise<void> {
  try {
    await deckRefreshBulk();
  } catch {
    // Errors surface via deck:ingest_error → ingest().error.
  }
  await refreshStatus();
}

type Mode = "loading" | "first-run" | "running" | "error" | "idle";

const DeckDataStrip: Component = () => {
  const mode = createMemo<Mode>(() => {
    const ing = ingest();
    const st = status();
    if (ing.error !== null) return "error";
    if (ing.running) return st !== null && st.oracles === 0 ? "first-run" : "running";
    if (st === null) return "loading";
    if (st.oracles === 0) return "first-run";
    return "idle";
  });

  const pct = createMemo(() => {
    const ing = ingest();
    if (ing.total <= 0) return 0;
    return Math.min(100, (ing.done / ing.total) * 100);
  });

  return (
    <div class={`decks-strip decks-strip--${mode()}`}>
      <div class="decks-strip-main">
        <Show when={mode() === "loading"}>
          <span class="decks-strip-title">Card data: loading…</span>
        </Show>

        <Show when={mode() === "idle"}>
          <span class="decks-strip-title">
            Card data: {status()!.oracles.toLocaleString()} oracles
          </span>
          <span class="decks-strip-sub">updated {formatDate(status()!.bulk_updated_at)}</span>
        </Show>

        <Show when={mode() === "first-run"}>
          <span class="decks-strip-title">
            Downloading Scryfall card data
          </span>
          <span class="decks-strip-sub">
            ≈31 MB · one-time · refreshed daily
          </span>
        </Show>

        <Show when={mode() === "running"}>
          <span class="decks-strip-title">{ingest().stage || "Refreshing card data…"}</span>
          <Show when={ingest().total > 0}>
            <span class="decks-strip-sub">
              {ingest().done.toLocaleString()} / {ingest().total.toLocaleString()}
            </span>
          </Show>
        </Show>

        <Show when={mode() === "error"}>
          <span class="decks-strip-title">Card data update failed</span>
          <span class="decks-strip-sub">{ingest().error}</span>
        </Show>
      </div>

      <Show when={mode() === "first-run" || mode() === "running"}>
        <div class="decks-strip-progress">
          <div class="decks-strip-progress-bar" style={{ width: `${pct()}%` }} />
        </div>
      </Show>

      <button
        class="decks-strip-btn"
        onClick={() => void onRefresh()}
        disabled={ingest().running}
      >
        {mode() === "error" ? "Retry" : "Refresh"}
      </button>
    </div>
  );
};

export default DeckDataStrip;
