// SetupControls — the granular build-parameter controls, extracted from the
// old SetupRail so SetupModal can render the full set inside a modal while
// SetupRail itself stays compact.

import { type Component, For, Show, createSignal, createMemo } from "solid-js";
import {
  format,
  setFormat,
  ownership,
  setOwnership,
  playstyle,
  setPlaystyle,
  colorSubset,
  setColorSubset,
  themeTags,
  setThemeTags,
  templateOverrides,
  setTemplateOverrides,
  templates,
  candidates,
  community,
} from "../../stores/deckStore";
import { inventory } from "../../stores/economyStore";
import type { Format, OwnershipMode, Playstyle, Role, Template, TemplateOverrides, WildcardBudget } from "../../lib/tauri";

export const WC_ICONS: Record<keyof WildcardBudget, string> = {
  common: "/icons/ObjectiveIcon_Wildcard_Common.png",
  uncommon: "/icons/ObjectiveIcon_Wildcard_Uncommon.png",
  rare: "/icons/ObjectiveIcon_Wildcard_Rare.png",
  mythic: "/icons/ObjectiveIcon_Wildcard_MythicRare.png",
};

export const COLORS: { letter: string; hex: string; text: string }[] = [
  { letter: "W", hex: "#f9faf4", text: "#111" },
  { letter: "U", hex: "#0e68ab", text: "#fff" },
  { letter: "B", hex: "#150b00", text: "#fff" },
  { letter: "R", hex: "#d3202a", text: "#fff" },
  { letter: "G", hex: "#00733e", text: "#fff" },
];

export const PLAYSTYLES: Playstyle[] = ["aggro", "midrange", "control"];

const ROLES: Role[] = [
  "land", "ramp", "draw", "removal", "wipe", "counter",
  "recursion", "tutor", "protection", "threat", "finisher", "payoff", "utility",
];

export function ownedWc(k: keyof WildcardBudget): number {
  const inv = inventory();
  if (inv === null) return 0;
  switch (k) {
    case "common": return inv.wc_common;
    case "uncommon": return inv.wc_uncommon;
    case "rare": return inv.wc_rare;
    case "mythic": return inv.wc_mythic;
  }
}

export function toggleColor(letter: string): void {
  const cur = colorSubset() ?? "";
  const has = cur.includes(letter);
  const set = new Set(cur.split(""));
  if (has) set.delete(letter); else set.add(letter);
  const next = "WUBRG".split("").filter((l) => set.has(l)).join("");
  setColorSubset(next.length === 0 ? null : next);
}

export const FormatControl: Component = () => (
  <div class="decks-field">
    <div class="decks-field-label">Format</div>
    <div class="decks-segmented">
      <For each={[{ v: "brawl100" as Format, label: "Brawl 100" }, { v: "standardbrawl60" as Format, label: "Standard Brawl 60" }]}>
        {(opt) => (
          <button
            class={`decks-seg ${format() === opt.v ? "decks-seg--active" : ""}`}
            onClick={() => setFormat(opt.v)}
          >
            {opt.label}
          </button>
        )}
      </For>
    </div>
  </div>
);

export const OwnershipControl: Component = () => (
  <div class="decks-field">
    <div class="decks-field-label">Ownership</div>
    <div class="decks-segmented">
      <button
        class={`decks-seg ${ownership().mode === "owned_only" ? "decks-seg--active" : ""}`}
        onClick={() => setOwnership({ mode: "owned_only", wc_budget: { common: 0, uncommon: 0, rare: 0, mythic: 0 } })}
      >
        Owned only
      </button>
      <button
        class={`decks-seg ${ownership().mode === "best_deck" ? "decks-seg--active" : ""}`}
        onClick={() => setOwnership({ mode: "best_deck" } as OwnershipMode)}
      >
        Best deck
      </button>
    </div>
  </div>
);

export const WildcardBudgetControl: Component = () => {
  function setBudget(k: keyof WildcardBudget, v: number): void {
    const o = ownership();
    if (o.mode !== "owned_only") return;
    const owned = ownedWc(k);
    const next = Math.max(0, Math.min(owned, v));
    setOwnership({ ...o, wc_budget: { ...o.wc_budget, [k]: next } });
  }

  const budget = createMemo<WildcardBudget>(() => {
    const o = ownership();
    return o.mode === "owned_only" ? o.wc_budget : { common: 0, uncommon: 0, rare: 0, mythic: 0 };
  });

  return (
    <div class="decks-field">
      <div class="decks-field-label">Wildcard budget</div>
      <div class="decks-wc-grid">
        <For each={["common", "uncommon", "rare", "mythic"] as (keyof WildcardBudget)[]}>
          {(k) => (
            <div class="decks-wc-row">
              <img class="decks-wc-icon" src={WC_ICONS[k]} alt={k} />
              <button class="decks-step" onClick={() => setBudget(k, budget()[k] - 1)}>−</button>
              <input
                class="decks-step-input"
                type="number"
                min="0"
                max={ownedWc(k)}
                value={budget()[k]}
                onInput={(e) => setBudget(k, Number((e.currentTarget as HTMLInputElement).value) || 0)}
              />
              <button class="decks-step" onClick={() => setBudget(k, budget()[k] + 1)}>+</button>
              <span class="decks-wc-owned">of {ownedWc(k)}</span>
            </div>
          )}
        </For>
      </div>
    </div>
  );
};

export const PlaystyleControl: Component = () => (
  <div class="decks-field">
    <div class="decks-field-label">Playstyle</div>
    <div class="decks-chips">
      <For each={PLAYSTYLES}>
        {(p) => (
          <button
            class={`decks-chip ${playstyle() === p ? "decks-chip--active" : ""}`}
            onClick={() => setPlaystyle(p)}
          >
            {p}
          </button>
        )}
      </For>
    </div>
  </div>
);

export const ColorPipsControl: Component = () => (
  <div class="decks-field">
    <div class="decks-field-label">Colours</div>
    <div class="decks-pips">
      <For each={COLORS}>
        {(c) => {
          const active = () => (colorSubset() ?? "").includes(c.letter);
          return (
            <button
              class={`decks-pip ${active() ? "decks-pip--active" : ""}`}
              style={{
                "background": c.hex,
                "color": c.text,
                "border-color": active() ? "var(--color-accent)" : "var(--color-border)",
              }}
              onClick={() => toggleColor(c.letter)}
              title={c.letter}
            >
              {c.letter}
            </button>
          );
        }}
      </For>
    </div>
  </div>
);

export const ThemeTagsControl: Component = () => {
  const [text, setText] = createSignal("");
  const suggestions = createMemo(() => {
    const q = text().toLowerCase().trim();
    const source = new Set<string>();
    for (const c of candidates()) for (const t of c.tags) source.add(t);
    const com = community();
    if (com) for (const e of com.edhrec) source.add(e.section);
    const active = new Set(themeTags());
    const list = Array.from(source).filter((t) => !active.has(t) && (q === "" || t.toLowerCase().includes(q)));
    list.sort();
    return list.slice(0, 12);
  });

  function add(tag: string): void {
    if (themeTags().includes(tag)) return;
    setThemeTags([...themeTags(), tag]);
    setText("");
  }

  function remove(tag: string): void {
    setThemeTags(themeTags().filter((t) => t !== tag));
  }

  return (
    <div class="decks-field">
      <div class="decks-field-label">Theme tags</div>
      <div class="decks-tags-selected">
        <For each={themeTags()}>
          {(t) => (
            <span class="decks-tag decks-tag--active" onClick={() => remove(t)}>
              {t} ×
            </span>
          )}
        </For>
      </div>
      <input
        class="decks-tag-input"
        placeholder="Type to filter tags…"
        value={text()}
        onInput={(e) => setText((e.currentTarget as HTMLInputElement).value)}
      />
      <Show when={suggestions().length > 0}>
        <div class="decks-tag-suggest">
          <For each={suggestions()}>
            {(t) => (
              <span class="decks-tag" onClick={() => add(t)}>
                {t}
              </span>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

export const AdvancedControl: Component<{ template: Template | undefined; alwaysOpen?: boolean }> = (props) => {
  const [open, setOpen] = createSignal(props.alwaysOpen === true);

  function overrideLands(v: number | null): void {
    const cur: TemplateOverrides = templateOverrides() ?? {};
    setTemplateOverrides({ ...cur, lands: v });
  }

  function overrideRole(kind: "role_min" | "role_max", role: Role, v: number | null): void {
    const cur: TemplateOverrides = templateOverrides() ?? {};
    const bucket = { ...(cur[kind] ?? {}) };
    if (v === null) delete bucket[role]; else bucket[role] = v;
    setTemplateOverrides({ ...cur, [kind]: bucket });
  }

  const tpl = () => props.template;
  const overrides = () => templateOverrides() ?? {};
  const isOpen = () => props.alwaysOpen === true || open();

  return (
    <div class="decks-field">
      <Show when={props.alwaysOpen !== true}>
        <button class="decks-disclosure" onClick={() => setOpen(!open())}>
          <span>{open() ? "▾" : "▸"} Advanced (template overrides)</span>
        </button>
      </Show>
      <Show when={isOpen() && tpl() !== undefined}>
        <div class="decks-advanced">
          <div class="decks-adv-row">
            <label>Lands</label>
            <input
              type="number"
              class="decks-adv-input"
              min="0"
              placeholder={String(tpl()!.lands)}
              value={overrides().lands ?? ""}
              onInput={(e) => {
                const raw = (e.currentTarget as HTMLInputElement).value;
                overrideLands(raw === "" ? null : Number(raw));
              }}
            />
          </div>
          <div class="decks-adv-section">Role min / max</div>
          <For each={ROLES}>
            {(role) => {
              const defMin = () => tpl()!.role_min[role] ?? 0;
              const defMax = () => tpl()!.role_max[role] ?? 0;
              const ovMin = () => overrides().role_min?.[role];
              const ovMax = () => overrides().role_max?.[role];
              return (
                <div class="decks-adv-row">
                  <label>{role}</label>
                  <input
                    type="number"
                    class="decks-adv-input decks-adv-input--sm"
                    min="0"
                    placeholder={String(defMin())}
                    value={ovMin() ?? ""}
                    onInput={(e) => {
                      const raw = (e.currentTarget as HTMLInputElement).value;
                      overrideRole("role_min", role, raw === "" ? null : Number(raw));
                    }}
                  />
                  <input
                    type="number"
                    class="decks-adv-input decks-adv-input--sm"
                    min="0"
                    placeholder={String(defMax())}
                    value={ovMax() ?? ""}
                    onInput={(e) => {
                      const raw = (e.currentTarget as HTMLInputElement).value;
                      overrideRole("role_max", role, raw === "" ? null : Number(raw));
                    }}
                  />
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
};

export function activeTemplate(): Template | undefined {
  const list = templates();
  return list.find((t) => t.format === format() && t.playstyle === playstyle());
}
