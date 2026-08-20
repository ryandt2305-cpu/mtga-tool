// Export popup — formats filtered collection in MTGA export format.
// Offers clipboard copy or .txt download.

import { createSignal, Show, onCleanup, type Component } from "solid-js";
import type { CollectionEntry } from "../stores/collectionStore";
import "./ExportPopup.css";
import Tooltip from "./Tooltip";

interface ExportPopupProps {
  cards: () => CollectionEntry[];
}

function formatMtgaExport(entries: CollectionEntry[]): string {
  return entries
    .map(
      (e) =>
        `${e.count} ${e.card.name} (${e.card.set_code.toUpperCase()}) ${e.card.collector_number}`
    )
    .join("\n");
}

const ExportPopup: Component<ExportPopupProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  const [copied, setCopied] = createSignal(false);
  let containerRef!: HTMLDivElement;

  function handleClickOutside(e: MouseEvent) {
    if (open() && containerRef && !containerRef.contains(e.target as Node)) {
      setOpen(false);
    }
  }

  document.addEventListener("mousedown", handleClickOutside);
  onCleanup(() =>
    document.removeEventListener("mousedown", handleClickOutside)
  );

  async function copyToClipboard() {
    const text = formatMtgaExport(props.cards());
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => {
      setCopied(false);
      setOpen(false);
    }, 1200);
  }

  function downloadTxt() {
    const text = formatMtgaExport(props.cards());
    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "mtga-collection.txt";
    a.click();
    URL.revokeObjectURL(url);
    setOpen(false);
  }

  return (
    <div class="export-container" ref={containerRef}>
      <Tooltip text="Export visible cards">
        <button
          class="export-btn"
          onClick={() => setOpen(!open())}
        >
          Export
        </button>
      </Tooltip>
      <Show when={open()}>
        <div class="export-popup">
          <button class="export-popup-option" onClick={copyToClipboard}>
            {copied() ? "Copied!" : "Copy to Clipboard"}
          </button>
          <button class="export-popup-option" onClick={downloadTxt}>
            Download .txt
          </button>
        </div>
      </Show>
    </div>
  );
};

export default ExportPopup;
