/* @refresh reload */
import { render } from "solid-js/web";
import "./styles/global.css";

function paintBootError(where: string, err: unknown): void {
  const root = document.getElementById("root") ?? document.body;
  const msg = err instanceof Error ? `${err.name}: ${err.message}\n\n${err.stack ?? ""}` : String(err);
  root.innerHTML = `<pre style="color:#f88;background:#1a0a0a;padding:16px;margin:0;white-space:pre-wrap;font:12px monospace;height:100vh;overflow:auto;">[boot ${where}]\n${msg}</pre>`;
  console.error(`[boot ${where}]`, err);
}
window.addEventListener("error", (e) => paintBootError("uncaught", e.error ?? e.message));
window.addEventListener("unhandledrejection", (e) => paintBootError("unhandled-rejection", e.reason));

import App from "./App";
import { initAppStore } from "./stores/appStore";
import { initCollectionStore } from "./stores/collectionStore";
import { initEconomyStore } from "./stores/economyStore";
import { initFeedStore } from "./stores/feedStore";
import { initEventsStore } from "./stores/eventsStore";
import { initPriceStore } from "./stores/priceStore";
import { initCosmeticsStore } from "./stores/cosmeticsStore";
import { initMasteryStore } from "./stores/masteryStore";
import { initConnectionStore } from "./stores/connectionStore";
import { initHistoryStore } from "./stores/historyStore";
import { initImageCacheStore } from "./stores/imageCacheStore";
import { initDeckStore } from "./stores/deckStore";
import * as tauri from "./lib/tauri";
import { fetchSetNames } from "./lib/scryfall";

// Expose tauri bridge on window for devtools testing
(window as any).tauri = tauri;

fetchSetNames(); // Fire-and-forget — populates set name cache for tooltips
initAppStore();
initCollectionStore();
initEconomyStore();
initEventsStore();
initFeedStore();
initConnectionStore();
initHistoryStore();
initImageCacheStore();
initPriceStore();
initCosmeticsStore();
initMasteryStore();
initDeckStore();

const root = document.getElementById("root");
if (!root) {
  paintBootError("no-root", new Error("#root element missing from index.html"));
} else {
  try {
    render(() => <App />, root);
  } catch (err) {
    paintBootError("render", err);
  }
}
