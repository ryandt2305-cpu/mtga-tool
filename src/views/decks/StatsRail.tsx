// StatsRail — right-hand pane summarising the current BuildResult.
// Curve + colour pips + role coverage are always visible ("at a glance").
// Ownership, wildcard cost, warnings, and community collapse into
// disclosure rows with a one-line summary you can read without expanding.

import { type Component, For, Show, createMemo } from "solid-js";
import EconomyChart from "../../components/EconomyChart";
import type { Role, WildcardBudget } from "../../lib/tauri";
import { community, result } from "../../stores/deckStore";
import { allCards, cardsByName, collection } from "../../stores/collectionStore";
import Disclosure from "./Disclosure";
import { WC_ICON } from "./wildcardIcons";
import CraftAdvisor from "./CraftAdvisor";
import ManaSymbol, { type ManaLetter } from "../../components/ManaSymbol";
import Tooltip from "../../components/Tooltip";

const CURVE_LABELS = ["0", "1", "2", "3", "4", "5", "6", "7+"];

const PIP_LETTERS: ManaLetter[] = ["W", "U", "B", "R", "G"];

const ROLE_ORDER: Role[] = [
  "land", "ramp", "draw", "removal", "wipe", "counter",
  "recursion", "tutor", "protection", "threat", "finisher", "payoff", "utility",
];

const SIGNALS_COPY = {
  liveCombos: "Every card in the combo is in your deck.",
  nearCombos: "One card away — Arena-legal, within commander colors.",
  noCombos: "No Spellbook combos matched this deck.",
  noEdhrec: "No EDHREC data for this commander.",
} as const;

const NEAR_COMBO_LIMIT = 5;

const ROLE_TOOLTIP: Record<Role, string> = {
  land: "Mana sources — the lands that cast your spells.",
  ramp: "Cards that produce extra mana faster than one land per turn.",
  draw: "Card draw and card advantage — refills your hand.",
  removal: "Single-target answers to opposing creatures or permanents.",
  wipe: "Board wipes — mass removal that clears multiple threats at once.",
  counter: "Counterspells — stop opponents' spells before they resolve.",
  recursion: "Return cards from your graveyard to hand or battlefield.",
  tutor: "Search your library for a specific card.",
  protection: "Protect your commander or key pieces (hexproof, indestructible, ward).",
  threat: "Cards that pressure the opponent's life total or board.",
  finisher: "High-impact spells that close out the game.",
  payoff: "Cards that reward the deck's theme (tribal, sacrifice, spellslinger…).",
  utility: "Flexible tools that don't fit a single role — filler for edge cases.",
};

const StatsRail: Component = () => (
  <aside class="decks-stats">
    <Show
      when={result() !== null}
      fallback={<div class="decks-empty-small">Stats appear after Build.</div>}
    >
      <CurveChart />
      <PipsRow />
      <RoleCoverage />

      <div class="decks-stats-disclosures">
        <SignalsDisclosure />
        <OwnershipDisclosure />
        <WildcardCostDisclosure />
        <CraftAdvisor />
        <WarningsDisclosure />
        <CommunityDisclosure />
      </div>
    </Show>
  </aside>
);

// ── Always-visible sections ───────────────────────────────────────

const CurveChart: Component = () => {
  const data = createMemo(() => {
    const s = result()!.stats;
    const c = s.curve.slice(0, 8);
    while (c.length < 8) c.push(0);
    return c;
  });
  return (
    <div class="decks-stats-section">
      <div class="decks-stats-h">Mana curve</div>
      <EconomyChart
        type="bar"
        labels={CURVE_LABELS}
        datasets={[{
          label: "cards",
          data: data(),
          borderColor: "#8f82ff",
          backgroundColor: "#8f82ff",
        }]}
        height={120}
      />
    </div>
  );
};

const PipsRow: Component = () => {
  const pips = () => result()!.stats.pips;
  return (
    <div class="decks-stats-section decks-pips-row">
      <For each={PIP_LETTERS}>
        {(letter, i) => (
          <div class="decks-pip-cell">
            <ManaSymbol letter={letter} size={18} />
            <span class="decks-pip-count">{pips()[i()] ?? 0}</span>
          </div>
        )}
      </For>
    </div>
  );
};

const RoleCoverage: Component = () => {
  const s = () => result()!.stats;
  const rows = createMemo(() => {
    const counts = s().role_counts;
    const targets = s().role_targets;
    return ROLE_ORDER
      .map((role) => {
        const c = counts[role] ?? 0;
        const tgt = targets[role];
        return { role, count: c, target: tgt };
      })
      .filter((row) => row.count > 0 || row.target !== undefined);
  });
  return (
    <div class="decks-stats-section">
      <div class="decks-stats-h">Roles</div>
      <ul class="decks-role-list">
        <For each={rows()}>
          {(row) => {
            const min = () => (row.target ? row.target[0] : 0);
            const max = () => (row.target ? row.target[1] : Math.max(row.count, 1));
            const below = () => row.count < min();
            const pct = () => Math.min(100, Math.round((row.count / Math.max(max(), 1)) * 100));
            return (
              <Tooltip text={ROLE_TOOLTIP[row.role]} contents>
                <li
                  class={`decks-role-row ${below() ? "decks-role-row--below" : ""}`}
                >
                  <span class="decks-role-label">{row.role}</span>
                  <span class="decks-role-bar"><span style={{ width: `${pct()}%` }} /></span>
                  <span class="decks-role-count">
                    {row.count}{row.target ? ` / ${min()}–${max()}` : ""}
                  </span>
                </li>
              </Tooltip>
            );
          }}
        </For>
      </ul>
    </div>
  );
};

// ── Disclosure sections ──────────────────────────────────────────

const SignalsDisclosure: Component = () => {
  const r = () => result()!;
  const c = () => community();

  const deckNamesLower = createMemo(() => {
    const set = new Set<string>();
    set.add(r().commander.name.toLowerCase());
    for (const s of r().slots) set.add(s.name.toLowerCase());
    return set;
  });

  const edhrecMap = createMemo(() => {
    const m = new Map<string, number>();
    const cc = c();
    if (!cc) return m;
    for (const e of cc.edhrec) m.set(e.name.toLowerCase(), e.inclusion);
    return m;
  });

  const commanderCI = createMemo<Set<string>>(() => {
    const card = allCards()[r().commander.grp_id];
    return new Set(card?.color_identity ?? []);
  });

  const liveCombos = createMemo(() => {
    const cc = c();
    if (!cc || cc.combos.length === 0) return [] as string[][];
    const names = deckNamesLower();
    return cc.combos.filter((combo) => combo.length > 0 && combo.every((n) => names.has(n)));
  });

  type Suggest = { missing: string; combo: string[]; owned: boolean; incl: number };
  const nearComboSuggestions = createMemo<Suggest[]>(() => {
    const cc = c();
    if (!cc || cc.combos.length === 0) return [];
    const names = deckNamesLower();
    const ci = commanderCI();
    const cardsIdx = cardsByName();
    const col = collection();
    const inc = edhrecMap();
    const rows: Suggest[] = [];
    for (const combo of cc.combos) {
      if (combo.length < 2) continue;
      let missing: string | null = null;
      let missingCount = 0;
      for (const n of combo) {
        if (!names.has(n)) {
          missing = n;
          missingCount++;
          if (missingCount > 1) break;
        }
      }
      if (missingCount !== 1 || missing === null) continue;
      const printings = cardsIdx.get(missing);
      if (!printings || printings.length === 0) continue;
      const candidateCI = printings[0].color_identity;
      if (!candidateCI.every((cc2) => ci.has(cc2))) continue;
      const owned = printings.some((p) => (col[p.grp_id] ?? 0) > 0);
      rows.push({ missing, combo, owned, incl: inc.get(missing) ?? 0 });
    }
    rows.sort((a, b) => {
      if (a.owned !== b.owned) return a.owned ? -1 : 1;
      return b.incl - a.incl;
    });
    return rows.slice(0, NEAR_COMBO_LIMIT);
  });

  const alignment = createMemo(() => {
    const cc = c();
    if (!cc || !cc.available || cc.edhrec.length === 0) return null;
    const eSet = new Set(cc.edhrec.map((e) => e.name.toLowerCase()));
    const commanderGrp = r().commander.grp_id;
    let denom = 0;
    let num = 0;
    for (const s of r().slots) {
      if (s.role === "land") continue;
      if (s.grp_id === commanderGrp) continue;
      denom++;
      if (eSet.has(s.name.toLowerCase())) num++;
    }
    if (denom === 0) return null;
    return { num, denom, pct: Math.round((num / denom) * 100) };
  });

  const shapeGaps = createMemo(() => {
    const stats = r().stats;
    let gaps = 0;
    for (const key of Object.keys(stats.role_targets) as Role[]) {
      const target = stats.role_targets[key];
      if (!target) continue;
      const count = stats.role_counts[key] ?? 0;
      if (count < target[0] || count > target[1]) gaps++;
    }
    return gaps;
  });

  function displayName(lower: string): string {
    if (r().commander.name.toLowerCase() === lower) return r().commander.name;
    for (const s of r().slots) {
      if (s.name.toLowerCase() === lower) return s.name;
    }
    const printings = cardsByName().get(lower);
    if (printings && printings.length > 0) return printings[0].name;
    return lower.replace(/\b\w/g, (m) => m.toUpperCase());
  }

  const tone = (): "default" | "warn" => (shapeGaps() > 0 ? "warn" : "default");

  const combosTip = () => {
    const n = liveCombos().length;
    const near = nearComboSuggestions().length;
    if (n === 0 && near === 0) return "0 combos complete. None are one card away either.";
    if (n === 0) return `0 combos complete. ${near} ${near === 1 ? "is" : "are"} one card away below.`;
    return `${n} combo${n === 1 ? "" : "s"} complete in this deck.`;
  };

  const alignTip = () => {
    const a = alignment();
    if (!a) return "No EDHREC data for this commander.";
    return `${a.pct}% of non-land picks (${a.num}/${a.denom}) also in EDHREC's list.`;
  };

  const shapeTip = () => {
    const n = shapeGaps();
    if (n === 0) return "Every role hits its template target.";
    return `${n} role${n === 1 ? "" : "s"} miss target. See Roles above.`;
  };

  const summary = () => (
    <span class="decks-signals-chips">
      <Tooltip text={combosTip()}><span>Combos {liveCombos().length}</span></Tooltip>
      <Tooltip text={alignTip()}>
        <span>Align {alignment() === null ? "—" : `${alignment()!.pct}%`}</span>
      </Tooltip>
      <Tooltip text={shapeTip()}>
        <span>Shape {shapeGaps() === 0 ? "✓" : `${shapeGaps()} gap${shapeGaps() === 1 ? "" : "s"}`}</span>
      </Tooltip>
    </span>
  );

  return (
    <Disclosure label="Signals" tone={tone()} summary={summary}>
      <div class="decks-signals-section">
        <div class="decks-stats-h">Live combos</div>
        <Show
          when={liveCombos().length > 0}
          fallback={<div class="decks-stats-sub">{SIGNALS_COPY.noCombos}</div>}
        >
          <div class="decks-stats-sub">{SIGNALS_COPY.liveCombos}</div>
          <ul class="decks-signals-combos">
            <For each={liveCombos()}>
              {(combo, i) => (
                <li class="decks-signals-combo-card">
                  <div class="decks-signals-combo-head">Combo {i() + 1}</div>
                  <div class="decks-signals-combo-cards">
                    <For each={combo}>
                      {(n, j) => (
                        <>
                          <Show when={j() > 0}>
                            <span class="decks-signals-plus">+</span>
                          </Show>
                          <span class="decks-signals-chip">{displayName(n)}</span>
                        </>
                      )}
                    </For>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>

      <Show when={nearComboSuggestions().length > 0}>
        <div class="decks-signals-section">
          <div class="decks-stats-h">Nearly there</div>
          <div class="decks-stats-sub">{SIGNALS_COPY.nearCombos}</div>
          <ul class="decks-signals-combos">
            <For each={nearComboSuggestions()}>
              {(row) => (
                <li class="decks-signals-combo-card decks-signals-combo-card--suggest">
                  <div class="decks-signals-combo-head">
                    <span>Add</span>
                    <Show when={row.owned}>
                      <Tooltip text="You own this card">
                        <span class="decks-signals-owned-pip" />
                      </Tooltip>
                    </Show>
                  </div>
                  <div class="decks-signals-combo-cards">
                    <span class="decks-signals-chip decks-signals-chip--add">
                      {displayName(row.missing)}
                    </span>
                    <span class="decks-signals-plus">→</span>
                    <For each={row.combo.filter((n) => n !== row.missing)}>
                      {(n, j) => (
                        <>
                          <Show when={j() > 0}>
                            <span class="decks-signals-plus">+</span>
                          </Show>
                          <span class="decks-signals-chip decks-signals-chip--have">
                            {displayName(n)}
                          </span>
                        </>
                      )}
                    </For>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </div>
      </Show>

      <div class="decks-signals-section">
        <div class="decks-stats-h">EDHREC alignment</div>
        <Show
          when={alignment() !== null}
          fallback={<div class="decks-stats-sub">{SIGNALS_COPY.noEdhrec}</div>}
        >
          <div class="decks-stats-sub">
            {alignment()!.num} of {alignment()!.denom} non-land picks match EDHREC ({alignment()!.pct}%).
          </div>
        </Show>
      </div>
    </Disclosure>
  );
};

const OwnershipDisclosure: Component = () => {
  const s = () => result()!.stats;
  const pct = () => Math.round((s().owned / Math.max(s().total, 1)) * 100);
  return (
    <Disclosure
      label="Ownership"
      summary={() => <span>{s().owned}/{s().total} · {pct()}%</span>}
    >
      <div class="decks-ownership-detail">
        <div class="decks-ownership-bar">
          <span style={{ width: `${pct()}%` }} />
        </div>
        <div class="decks-stats-sub">
          {s().owned} owned · {s().total - s().owned} missing
        </div>
      </div>
    </Disclosure>
  );
};

const WildcardCostDisclosure: Component = () => {
  const cost = () => result()!.stats.wildcard_cost;
  const items: (keyof WildcardBudget)[] = ["common", "uncommon", "rare", "mythic"];
  const total = () => items.reduce((n, k) => n + (cost()[k] ?? 0), 0);

  const summary = () => {
    if (total() === 0) return <span class="decks-stats-sub">None — fully owned</span>;
    return (
      <span class="decks-wc-row decks-wc-row--summary">
        <For each={items}>
          {(k) => (
            <Show when={(cost()[k] ?? 0) > 0}>
              <span class="decks-wc-chip decks-wc-chip--compact">
                <img class="decks-wc-icon" src={WC_ICON[k]} alt={k} />
                <span>{cost()[k]}</span>
              </span>
            </Show>
          )}
        </For>
      </span>
    );
  };

  return (
    <Disclosure label="Wildcard cost" summary={summary}>
      <div class="decks-wc-row">
        <For each={items}>
          {(k) => (
            <span class="decks-wc-chip">
              <img class="decks-wc-icon" src={WC_ICON[k]} alt={k} />
              <span>{cost()[k] ?? 0}</span>
            </span>
          )}
        </For>
      </div>
      <div class="decks-stats-sub">Total {total()} wildcard{total() === 1 ? "" : "s"}.</div>
    </Disclosure>
  );
};

const WarningsDisclosure: Component = () => {
  const w = () => result()!.warnings;
  return (
    <Show when={w().length > 0}>
      <Disclosure
        label="Warnings"
        tone="warn"
        defaultOpen
        summary={() => <span>{w().length} issue{w().length === 1 ? "" : "s"}</span>}
      >
        <ul class="decks-warn-list">
          <For each={w()}>
            {(warn) => (
              <li class="decks-warn-row">
                <span class="decks-warn-code">{warn.code}</span>
                <span class="decks-warn-msg">{warn.message}</span>
              </li>
            )}
          </For>
        </ul>
      </Disclosure>
    </Show>
  );
};

const CommunityDisclosure: Component = () => {
  const c = () => community();
  const available = () => c() !== null && c()!.available;
  const summary = () => {
    if (!available()) return <span class="decks-stats-sub">offline build</span>;
    const d = c()!;
    return (
      <span class="decks-stats-sub">
        EDHREC {d.edhrec.length > 0 ? "✓" : "—"} · combos {d.combos.length > 0 ? "✓" : "—"}
      </span>
    );
  };
  return (
    <Disclosure label="Community" summary={summary}>
      <Show
        when={available()}
        fallback={<div class="decks-stats-sub">Community data unavailable — built offline.</div>}
      >
        <div class="decks-stats-sub">EDHREC recommendations: {c()!.edhrec.length}</div>
        <div class="decks-stats-sub">Known combos: {c()!.combos.length}</div>
      </Show>
    </Disclosure>
  );
};

export default StatsRail;
