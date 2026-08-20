// Economy dashboard — tabbed layout: Currencies | Cosmetics | Mastery | History.
// Data comes from economyStore, cosmeticsStore, masteryStore, and historyStore.

import { type Component, Show, For, createSignal } from "solid-js";
import Header from "../components/Header";
import { inventory, hasInventory } from "../stores/economyStore";
import { hasCosmetics } from "../stores/cosmeticsStore";
import { hasMastery } from "../stores/masteryStore";
import CosmeticsTab from "./economy/CosmeticsTab";
import MasteryTab from "./economy/MasteryTab";
import HistoryTab from "./economy/HistoryTab";
import SetsTab from "./economy/SetsTab";
import { hasHistory } from "../stores/historyStore";
import type { BoosterStack } from "../lib/tauri";
import { scryfallSetSvg, boosterIconUrl } from "../lib/scryfall";

// --- Tab State ---

type EconomyTab = "currencies" | "cosmetics" | "mastery" | "history" | "sets";
const [activeTab, setActiveTab] = createSignal<EconomyTab>("currencies");

// --- Helpers ---

function formatNumber(n: number): string {
  return n.toLocaleString("en-US");
}

function sortedBoosters(boosters: BoosterStack[]): BoosterStack[] {
  return [...boosters]
    .filter((b) => b.count > 0)
    .sort((a, b) => b.count - a.count || (a.set_code ?? "").localeCompare(b.set_code ?? ""));
}

function iconFallback(letter: string) {
  return (e: Event) => {
    const img = e.target as HTMLImageElement;
    img.style.display = "none";
    const parent = img.parentElement;
    if (parent && !parent.querySelector("span")) {
      const span = document.createElement("span");
      span.textContent = letter;
      parent.appendChild(span);
    }
  };
}

// --- Component ---

const Economy: Component = () => {
  return (
    <div class="view">
      <Header title="Economy" />
      <div class="view-content">
        <TabBar />
        <Show when={activeTab() === "currencies"}>
          <Show when={hasInventory()} fallback={<NoData />}>
            <CurrenciesSection />
            <WildcardsSection />
            <VaultSection />
            <BoostersSection />
            <TokensSection />
          </Show>
        </Show>
        <Show when={activeTab() === "cosmetics"}>
          <Show when={hasCosmetics()} fallback={<NoData />}>
            <CosmeticsTab />
          </Show>
        </Show>
        <Show when={activeTab() === "mastery"}>
          <Show when={hasMastery()} fallback={<NoData />}>
            <MasteryTab />
          </Show>
        </Show>
        <Show when={activeTab() === "history"}>
          <HistoryTab />
        </Show>
        <Show when={activeTab() === "sets"}>
          <SetsTab />
        </Show>
      </div>
    </div>
  );
};

// --- Tab Bar ---

const TabBar: Component = () => {
  const tabs: { id: EconomyTab; label: string }[] = [
    { id: "currencies", label: "Currencies" },
    { id: "cosmetics", label: "Cosmetics" },
    { id: "mastery", label: "Mastery" },
    { id: "history", label: "History" },
    { id: "sets", label: "Sets" },
  ];

  return (
    <div class="economy-tabs">
      <For each={tabs}>
        {(tab) => (
          <button
            class={`economy-tab ${activeTab() === tab.id ? "economy-tab--active" : ""}`}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        )}
      </For>
    </div>
  );
};

// --- Currencies Tab Sections ---

const NoData: Component = () => (
  <div class="scan-prompt">
    <p class="scan-prompt-text">
      No data yet. Start MTGA and wait for the session to load, or
      start the log watcher from Settings.
    </p>
  </div>
);

const WILDCARD_ICONS: Record<string, string> = {
  common: "/icons/Nav_WildCard_Common.png",
  uncommon: "/icons/Nav_WildCard_Uncommon.png",
  rare: "/icons/Nav_WildCard_Rare.png",
  mythic: "/icons/Nav_WildCard_MythicRare.png",
};

const CUSTOM_TOKEN_ICONS: Record<string, string> = {
  JumpInToken: "/icons/ObjectiveIcon_JumpInToken.png",
  PlayInEventToken: "/icons/Nav_Token_PlayIn.png",
  QuickDraftToken: "/icons/Nav_Token_QuickDraft.png",
  ArenaDirect: "/icons/Nav_Token_ArenaDirect.png",
  QualifierToken: "/icons/ObjectiveIcon_TokenQualifier.png",
  InvitationToken: "/icons/Nav_Token_Invitation.png",
};

const SECTION_ICONS: Record<string, string> = {
  currencies: "/icons/Nav_Coins.png",
  wildcards: "/icons/Nav_WildCard_Glow.png",
  vault: "/icons/Nav_Vault.png",
  boosters: "/icons/ObjectiveIcon_Pack_Generic.png",
  tokens: "/icons/Nav_Token.png",
};

const CurrenciesSection: Component = () => {
  const inv = () => inventory()!;
  return (
    <section class="economy-section">
      <h2 class="economy-section-title">
        <img class="economy-section-icon" src={SECTION_ICONS.currencies} alt="" />
        Currencies
      </h2>
      <div class="economy-grid">
        <div class="stat-card">
          <div class="stat-card-icon" style={{ background: "transparent" }}>
            <img class="economy-icon" src="/icons/ObjectiveIcon_Gold.png" alt="" onError={iconFallback("G")} />
          </div>
          <span class="stat-card-value">{formatNumber(inv().gold)}</span>
          <span class="stat-card-label">Gold</span>
        </div>
        <div class="stat-card">
          <div class="stat-card-icon" style={{ background: "transparent" }}>
            <img class="economy-icon" src="/icons/ObjectiveIcon_Gem.png" alt="" onError={iconFallback("$")} />
          </div>
          <span class="stat-card-value">{formatNumber(inv().gems)}</span>
          <span class="stat-card-label">Gems</span>
        </div>
      </div>
    </section>
  );
};

const WildcardsSection: Component = () => {
  const inv = () => inventory()!;
  const wildcards = () => [
    { label: "Common", count: inv().wc_common, rarity: "common" },
    { label: "Uncommon", count: inv().wc_uncommon, rarity: "uncommon" },
    { label: "Rare", count: inv().wc_rare, rarity: "rare" },
    { label: "Mythic", count: inv().wc_mythic, rarity: "mythic" },
  ];

  return (
    <section class="economy-section">
      <h2 class="economy-section-title">
        <img class="economy-section-icon" src={SECTION_ICONS.wildcards} alt="" />
        Wildcards
      </h2>
      <div class="economy-grid">
        <For each={wildcards()}>
          {(wc) => (
            <div class={`stat-card wc-${wc.rarity}`}>
              <div class="stat-card-icon" style={{ background: "transparent" }}>
                <img class="economy-icon" src={WILDCARD_ICONS[wc.rarity]} alt="" />
              </div>
              <span class="stat-card-value">{wc.count}</span>
              <span class="stat-card-label">{wc.label}</span>
            </div>
          )}
        </For>
      </div>
    </section>
  );
};

const VaultSection: Component = () => {
  const inv = () => inventory()!;
  const pct = () => Math.min(inv().vault_progress, 100);
  const pctStr = () => inv().vault_progress.toFixed(1) + "%";

  return (
    <section class="economy-section">
      <h2 class="economy-section-title">
        <img class="economy-section-icon" src={SECTION_ICONS.vault} alt="" />
        Vault Progress
      </h2>
      <div class="vault-bar-container">
        <img class="vault-bar-icon" src="/icons/Nav_Vault.png" alt="" />
        <div class="vault-bar">
          <div class="vault-bar-fill" style={{ width: `${pct()}%` }} />
        </div>
        <span class="vault-percent">{pctStr()}</span>
      </div>
      <p class="vault-detail">
        <img class="vault-detail-icon" src={WILDCARD_ICONS.rare} alt="" />
        Track position {inv().wc_track_position} of 6
      </p>
    </section>
  );
};

const BoostersSection: Component = () => {
  const inv = () => inventory()!;
  const boosters = () => sortedBoosters(inv().boosters);

  return (
    <section class="economy-section">
      <h2 class="economy-section-title">
        <img class="economy-section-icon" src={SECTION_ICONS.boosters} alt="" />
        Boosters ({inv().total_boosters} total)
      </h2>
      <Show
        when={boosters().length > 0}
        fallback={<p class="stat-card-label">No boosters</p>}
      >
        <div class="booster-grid">
          <For each={boosters()}>
            {(b) => (
              <div class="booster-pill">
                <div class="booster-pill-icon">
                  <Show when={b.set_code} fallback={<span>?</span>}>
                    <img
                      src={boosterIconUrl(b.set_code!)}
                      alt={b.set_code ?? ""}
                      onError={(e) => {
                        const img = e.target as HTMLImageElement;
                        const fallbackSrc = scryfallSetSvg(b.set_code!);
                        if (img.src !== fallbackSrc) {
                          img.src = fallbackSrc;
                          img.classList.add("booster-pill-icon-svg");
                        } else {
                          img.style.display = "none";
                        }
                      }}
                    />
                  </Show>
                </div>
                <span class="booster-pill-code">{b.set_code ?? "???"}</span>
                <span class="booster-pill-count">&times;{b.count}</span>
              </div>
            )}
          </For>
        </div>
      </Show>
    </section>
  );
};

const TokensSection: Component = () => {
  const inv = () => inventory()!;
  const hasCustom = () => Object.keys(inv().custom_tokens).length > 0;

  return (
    <section class="economy-section">
      <h2 class="economy-section-title">
        <img class="economy-section-icon" src={SECTION_ICONS.tokens} alt="" />
        Tokens
      </h2>
      <div class="economy-tokens">
        <span class="economy-token-item">
          <img class="economy-token-icon" src="/icons/ObjectiveIcon_PlayerDraftToken.png" alt="" />
          <span class="economy-token-value">{inv().draft_tokens}</span>
        </span>
        <span class="economy-token-separator">&middot;</span>
        <span class="economy-token-item">
          <img class="economy-token-icon" src="/icons/ObjectiveIcon_PIPToken.png" alt="" />
          <span class="economy-token-value">{inv().sealed_tokens}</span>
        </span>
        <Show when={hasCustom()}>
          <For each={Object.entries(inv().custom_tokens)}>
            {([name, count]) => (
              <>
                <span class="economy-token-separator">&middot;</span>
                <span class="economy-token-item">
                  {CUSTOM_TOKEN_ICONS[name]
                    ? <img class="economy-token-icon" src={CUSTOM_TOKEN_ICONS[name]} alt="" />
                    : <span class="economy-token-label">{name}:</span>
                  }
                  <span class="economy-token-value">{count}</span>
                </span>
              </>
            )}
          </For>
        </Show>
      </div>
    </section>
  );
};

export default Economy;
