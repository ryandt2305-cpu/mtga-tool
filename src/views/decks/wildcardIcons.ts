// Wildcard rarity → extracted MTGA icon (mandatory: no emoji/placeholders).

import type { Rarity, WildcardBudget } from "../../lib/tauri";

export const WC_ICON: Record<keyof WildcardBudget, string> = {
  common: "/icons/ObjectiveIcon_Wildcard_Common.png",
  uncommon: "/icons/ObjectiveIcon_Wildcard_Uncommon.png",
  rare: "/icons/ObjectiveIcon_Wildcard_Rare.png",
  mythic: "/icons/ObjectiveIcon_Wildcard_MythicRare.png",
};

export function wcIconForRarity(r: Rarity): string | null {
  if (r === "common" || r === "uncommon" || r === "rare" || r === "mythic") {
    return WC_ICON[r];
  }
  return null;
}
