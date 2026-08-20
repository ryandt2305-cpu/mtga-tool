// Scryfall URL helpers — shared across Economy views.

export function scryfallSetSvg(setCode: string): string {
  return `https://svgs.scryfall.io/sets/${setCode.toLowerCase()}.svg`;
}

export function boosterIconUrl(setCode: string): string {
  return `/icons/BoosterPack_${setCode.toUpperCase()}.png`;
}

// --- Set name resolution ---

/** Dynamically fetched set names (populated by fetchSetNames). */
const setNameCache = new Map<string, string>();
let fetchAttempted = false;

/**
 * Fetch all set names from Scryfall's bulk sets endpoint.
 * Called once on app init — populates the cache for setDisplayName().
 * Silently no-ops on failure (tooltips fall back to uppercase codes).
 */
export async function fetchSetNames(): Promise<void> {
  if (fetchAttempted) return;
  fetchAttempted = true;

  try {
    let url: string | null = "https://api.scryfall.com/sets";
    while (url) {
      const resp: Response = await fetch(url);
      if (!resp.ok) break;
      const json: { data?: { code: string; name: string }[]; has_more?: boolean; next_page?: string } = await resp.json();
      for (const set of json.data ?? []) {
        if (set.code && set.name) {
          setNameCache.set(set.code.toLowerCase(), set.name);
        }
      }
      url = json.has_more ? (json.next_page ?? null) : null;
    }
  } catch {
    // Offline or rate-limited — setDisplayName falls back to code
  }
}

/** Resolve a set code to its full name. Falls back to uppercased code. */
export function setDisplayName(setCode: string): string {
  return setNameCache.get(setCode.toLowerCase()) ?? setCode.toUpperCase();
}
