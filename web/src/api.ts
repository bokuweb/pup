export type Fact = { key: string; label: string; text: string; value?: number };
export type MarketDef = { id: string; symbol: string; name: string; group: string; measure: "percent" | "points" | "basis_points"; wiki: boolean };
export type EventType = { id: string; name: string; description: string };
export type Meta = { generated_at: string; markets: MarketDef[]; event_types: EventType[]; horizons: number[]; n_events: number };
export type EventSummary = { id: string; type: string; title: string; date: string; time_utc: string; stage: string | null; facts: Fact[]; r1: Record<string, number> };
export type MarketWindow = { market: string; facts: Fact[]; path: [number, number][] };
export type EventDetail = {
  event: { id: string; type: string; title: string; date: string; time_utc: string; time_approx: boolean; facts: Fact[]; sources: { label: string; url: string }[] };
  stage: string | null;
  groups: [string, string][];
  markets: MarketWindow[];
};
export type Stats = { n: number; mean: number; median: number; p25: number; p75: number; up_share: number };
export type GroupAgg = { id: string; label: string; n_events: number; stats: Record<string, Record<string, Stats>>; path: Record<string, [number, number, number, number][]> };
export type TypeDetail = { type: EventType; n_events: number; groups: GroupAgg[] };
export type MarketDetail = { market: MarketDef; reactions: { event: string; type: string; title: string; date: string; post: Record<string, number> }[] };
export type WikiEntry = { path: string; type: string; title: string; description: string; date?: string; tags?: string[]; event_id?: string; event_type?: string; market?: string; has_analysis: boolean };
export type WikiPage = { path: string; frontmatter: Record<string, unknown>; body: string };
export type WikiGraph = { nodes: { id: string; type: string; title: string }[]; links: { source: string; target: string }[] };

const cache = new Map<string, Promise<unknown>>();

export function getJson<T>(path: string): Promise<T> {
  if (!cache.has(path)) {
    cache.set(
      path,
      fetch(`./${path}`).then((r) => {
        if (!r.ok) throw new Error(`${path}: HTTP ${r.status}`);
        return r.json();
      }),
    );
  }
  return cache.get(path) as Promise<T>;
}

export function unit(m: MarketDef): string {
  return m.measure === "percent" ? "%" : m.measure === "points" ? "pt" : "bp";
}

export function decimals(m: MarketDef): number {
  return m.measure === "basis_points" ? 1 : 2;
}

export function fmt(v: number | undefined, m: MarketDef): string {
  if (v === undefined || v === null || Number.isNaN(v)) return "–";
  const s = v.toFixed(decimals(m));
  const z = Number(s) === 0 ? s.replace("-", "") : s;
  return `${Number(z) > 0 ? "+" : ""}${z}${unit(m)}`;
}
