import {
  Area, Bar, BarChart, CartesianGrid, Cell, ComposedChart, LabelList, Legend, Line, LineChart,
  ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from "recharts";
import type { TooltipContentProps } from "recharts";

// Categorical slots in fixed order (validated palette). Color follows the entity within a chart.
export const SLOTS = ["var(--series-1)", "var(--series-2)", "var(--series-3)", "var(--series-4)"];

export type Series = { id: string; name: string; points: [number, number][] };

/** Fixed slot per market so a market keeps its color in every panel it appears in
 *  (the major indices never collide; each panel has at most four series). */
const MARKET_SLOT: Record<string, number> = {
  spx: 0, n225: 1, dax: 2, hsi: 3, ndx: 2, rut: 3, topix: 0, sx5e: 0, ftse: 1,
  us10y: 0, vix: 0, usdjpy: 0, dxy: 1, eurusd: 2, gbpusd: 3, gold: 3, wti: 1, btc: 2,
};

export function colorOf(id: string, i: number): string {
  return SLOTS[(MARKET_SLOT[id] ?? i) % SLOTS.length];
}

const axis = { stroke: "var(--text-muted)", fontSize: 11, tickLine: false, axisLine: false };

/** Diverging fill for a signed value: red ← gray → blue, saturating at ±scale. */
export function heat(v: number | undefined, scale: number): string {
  if (v === undefined || Number.isNaN(v)) return "transparent";
  const t = Math.min(1, Math.abs(v) / scale);
  const pct = Math.round(12 + t * 70);
  return `color-mix(in oklab, ${v >= 0 ? "var(--div-pos)" : "var(--div-neg)"} ${pct}%, var(--div-mid))`;
}

/** Heatmap cell colors: the border carries the tint, the fill is a faint wash of the same hue.
 *  Mixing with transparent (not gray) keeps mid-tones clean; near-zero moves almost disappear. */
export function tint(v: number | undefined, scale: number): { border: string; background: string } {
  if (v === undefined || Number.isNaN(v)) return { border: "transparent", background: "transparent" };
  const t = Math.min(1, Math.abs(v) / scale);
  const pole = v >= 0 ? "var(--div-pos)" : "var(--div-neg)";
  return {
    border: `color-mix(in srgb, ${pole} ${Math.round(8 + t * 62)}%, transparent)`,
    background: `color-mix(in srgb, ${pole} ${Math.round(2 + t * 16)}%, transparent)`,
  };
}

/** Scale at which a market's move counts as "large" for coloring. */
export function heatScale(unitStr: string): number {
  return unitStr === "bp" ? 12 : unitStr === "pt" ? 2.5 : 1.5;
}

function rows(series: Series[]) {
  const by = new Map<number, Record<string, number>>();
  for (const s of series) for (const [k, v] of s.points) by.set(k, { ...(by.get(k) ?? { k }), [s.id]: v });
  return [...by.values()].sort((a, b) => a.k - b.k);
}

function Tip({ active, payload, label, unit, names }: TooltipContentProps<number, string> & { unit: string; names: Record<string, string> }) {
  if (!active || !payload?.length) return null;
  return (
    <div className="tooltip">
      <div className="muted">{Number(label) === 0 ? "発表前の最後の引け" : Number(label) > 0 ? `発表後 ${label} 本目の引け` : `発表の ${-Number(label)} 本前の引け`}</div>
      {payload
        .filter((p) => typeof p.value === "number")
        .map((p) => (
          <div className="row" key={String(p.dataKey)}>
            <span><span className="swatch" style={{ background: p.color }} />{names[String(p.dataKey)] ?? p.dataKey}</span>
            <b>{(p.value as number) > 0 ? "+" : ""}{(p.value as number).toFixed(unit === "bp" ? 1 : 2)}{unit}</b>
          </div>
        ))}
    </div>
  );
}

/** Change from the last close before the event, by session offset (-20 … +20). */
export function PathChart({ series, unit, height = 220 }: { series: Series[]; unit: string; height?: number }) {
  if (series.length === 1) return <AreaPath s={series[0]} unit={unit} height={height} />;
  const data = rows(series);
  const names = Object.fromEntries(series.map((s) => [s.id, s.name]));
  const last = (id: string) => data.reduce((acc, r, i) => (r[id] !== undefined ? i : acc), -1);
  return (
    <ResponsiveContainer width="100%" height={height}>
      <LineChart data={data} margin={{ top: 8, right: 76, bottom: 4, left: 0 }}>
        <CartesianGrid stroke="var(--grid)" vertical={false} />
        <XAxis dataKey="k" type="number" domain={["dataMin", "dataMax"]} {...axis} ticks={[-20, -10, -5, 0, 5, 10, 20]} />
        <YAxis {...axis} width={48} tickFormatter={(v) => `${v}${unit}`} />
        <ReferenceLine x={0} stroke="var(--accent)" strokeOpacity={0.6} strokeDasharray="3 3" label={{ value: "発表", position: "insideTopLeft", fill: "var(--accent)", fontSize: 11 }} />
        <ReferenceLine y={0} stroke="var(--text-muted)" />
        <Tooltip content={(p) => <Tip {...(p as TooltipContentProps<number, string>)} unit={unit} names={names} />} />
        {series.length > 1 && <Legend wrapperStyle={{ fontSize: 12, color: "var(--text-secondary)" }} formatter={(v) => <span style={{ color: "var(--text-secondary)" }}>{v}</span>} />}
        {series.map((s, i) => (
          <Line
            key={s.id}
            dataKey={s.id}
            name={s.name}
            stroke={colorOf(s.id, i)}
            strokeWidth={2}
            dot={false}
            activeDot={{ r: 4 }}
            connectNulls
            isAnimationActive={false}
            label={series.length <= 4 ? (p: { index?: number; x?: number | string; y?: number | string }) => (p.index === last(s.id) ? <text x={Number(p.x ?? 0) + 6} y={Number(p.y ?? 0) + 4} fontSize={11} fill="var(--text-secondary)">{s.name.split("（")[0]}</text> : <g />) : undefined}
          />
        ))}
      </LineChart>
    </ResponsiveContainer>
  );
}

function AreaPath({ s, unit, height }: { s: Series; unit: string; height: number }) {
  const data = rows([s]);
  const gid = `g-${s.id}`;
  return (
    <ResponsiveContainer width="100%" height={height}>
      <ComposedChart data={data} margin={{ top: 8, right: 16, bottom: 4, left: 0 }}>
        <defs>
          <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={colorOf(s.id, 0)} stopOpacity={0.35} />
            <stop offset="100%" stopColor={colorOf(s.id, 0)} stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid stroke="var(--grid)" vertical={false} />
        <XAxis dataKey="k" type="number" domain={["dataMin", "dataMax"]} {...axis} ticks={[-20, -10, -5, 0, 5, 10, 20]} />
        <YAxis {...axis} width={48} tickFormatter={(v) => `${v}${unit}`} />
        <ReferenceLine x={0} stroke="var(--accent)" strokeOpacity={0.6} strokeDasharray="3 3" label={{ value: "発表", position: "insideTopLeft", fill: "var(--accent)", fontSize: 11 }} />
        <ReferenceLine y={0} stroke="var(--text-muted)" strokeOpacity={0.6} />
        <Tooltip content={(p) => <Tip {...(p as TooltipContentProps<number, string>)} unit={unit} names={{ [s.id]: s.name }} />} />
        <Area dataKey={s.id} stroke={colorOf(s.id, 0)} strokeWidth={2} fill={`url(#${gid})`} dot={false} activeDot={{ r: 4 }} isAnimationActive={false} />
      </ComposedChart>
    </ResponsiveContainer>
  );
}

/** Each market's move after the event as horizontal bars (blue up, red down), sorted. */
function niceCeil(x: number): number {
  if (!(x > 0)) return 1;
  const p = 10 ** Math.floor(Math.log10(x));
  return [1, 2, 2.5, 5, 10].map((k) => k * p).find((v) => v >= x) ?? 10 * p;
}

export function MoveBars({ data, height }: { data: { name: string; v: number; text: string; unit: string }[]; height?: number }) {
  const rowsSorted = [...data].sort((a, b) => b.v - a.v);
  // Symmetric, round limits with room for the value labels.
  const lim = niceCeil(Math.max(...rowsSorted.map((d) => Math.abs(d.v)), 0) * 1.35);
  const ticks = [-lim, -lim / 2, 0, lim / 2, lim];
  const single = rowsSorted.length === 1;
  return (
    <ResponsiveContainer width="100%" height={height ?? rowsSorted.length * 30 + 20}>
      <BarChart data={rowsSorted} layout="vertical" margin={{ top: 4, right: 16, bottom: 4, left: 8 }} barCategoryGap={6}>
        <CartesianGrid stroke="var(--grid)" horizontal={false} />
        {/* Symmetric around zero so a bar's direction reads at a glance, even with one bar. */}
        <XAxis type="number" {...axis} domain={[-lim, lim]} ticks={ticks} tickFormatter={(v) => `${v}`} />
        <YAxis type="category" dataKey="name" {...axis} width={single ? 0 : 120} hide={single} tick={{ fill: "var(--text-secondary)", fontSize: 12 }} />
        <ReferenceLine x={0} stroke="var(--text-muted)" />
        <Tooltip
          cursor={{ fill: "var(--grid)" }}
          content={({ active, payload }) => {
            const d = active ? (payload?.[0]?.payload as (typeof data)[number] | undefined) : undefined;
            return d ? <div className="tooltip"><div className="row"><span>{d.name}</span><b>{d.text}</b></div></div> : null;
          }}
        />
        <Bar dataKey="v" radius={4} isAnimationActive={false}>
          {rowsSorted.map((d, i) => <Cell key={i} fill={d.v >= 0 ? "var(--div-pos)" : "var(--div-neg)"} />)}
          <LabelList
            dataKey="text"
            content={(p) => {
              const { x = 0, y = 0, width = 0, height = 0, index = 0 } = p as { x?: number; y?: number; width?: number; height?: number; index?: number };
              const v = rowsSorted[index]?.v ?? 0;
              // Outside the bar's far end: right of positive bars, left of negative ones.
              const left = Math.min(Number(x), Number(x) + Number(width));
              const right = Math.max(Number(x), Number(x) + Number(width));
              return (
                <text x={v >= 0 ? right + 6 : left - 6} y={Number(y) + Number(height) / 2 + 4} textAnchor={v >= 0 ? "start" : "end"} fontSize={11} fill="var(--text-secondary)" fontFamily="JetBrains Mono, monospace">
                  {rowsSorted[index]?.text}
                </text>
              );
            }}
          />
        </Bar>
      </BarChart>
    </ResponsiveContainer>
  );
}

/** Mean path with the interquartile band, for one market and one sub-group. */
export function BandChart({ path, unit, name, height = 260 }: { path: [number, number, number, number][]; unit: string; name: string; height?: number }) {
  const data = path.map(([k, mean, p25, p75]) => ({ k, mean, band: [p25, p75] }));
  return (
    <ResponsiveContainer width="100%" height={height}>
      <ComposedChart data={data} margin={{ top: 8, right: 16, bottom: 4, left: 0 }}>
        <CartesianGrid stroke="var(--grid)" vertical={false} />
        <XAxis dataKey="k" type="number" domain={["dataMin", "dataMax"]} {...axis} ticks={[-20, -10, -5, 0, 5, 10, 20]} />
        <YAxis {...axis} width={48} tickFormatter={(v) => `${v}${unit}`} />
        <ReferenceLine x={0} stroke="var(--accent)" strokeOpacity={0.6} strokeDasharray="3 3" label={{ value: "発表", position: "insideTopLeft", fill: "var(--accent)", fontSize: 11 }} />
        <ReferenceLine y={0} stroke="var(--text-muted)" strokeOpacity={0.6} />
        <Tooltip
          content={({ active, payload, label }) =>
            active && payload?.length ? (
              <div className="tooltip">
                <div className="muted">{Number(label) > 0 ? `発表後 ${label} 本目` : Number(label) === 0 ? "発表前の最後の引け" : `${-Number(label)} 本前`}</div>
                <div className="row"><span>平均</span><b>{(payload.find((p) => p.dataKey === "mean")?.value as number)?.toFixed(2)}{unit}</b></div>
                <div className="row"><span>四分位（25〜75%）</span><b>{((payload.find((p) => p.dataKey === "band")?.value as number[]) ?? []).map((x) => x.toFixed(2)).join(" 〜 ")}{unit}</b></div>
              </div>
            ) : null
          }
        />
        <Area dataKey="band" stroke="none" fill="var(--band)" isAnimationActive={false} />
        <Line dataKey="mean" name={name} stroke="var(--series-1)" strokeWidth={2.5} dot={false} isAnimationActive={false} />
      </ComposedChart>
    </ResponsiveContainer>
  );
}

/** Reactions per event as bars, colored by event type. */
export function ReactionBars({ data, unit, typeIndex, typeNames, height = 260 }: {
  data: { title: string; date: string; type: string; v: number }[];
  unit: string;
  typeIndex: Record<string, number>;
  typeNames: Record<string, string>;
  height?: number;
}) {
  return (
    <ResponsiveContainer width="100%" height={height}>
      <BarChart data={data} margin={{ top: 8, right: 16, bottom: 4, left: 0 }} barCategoryGap={2}>
        <CartesianGrid stroke="var(--grid)" vertical={false} />
        <XAxis dataKey="date" {...axis} interval="preserveStartEnd" minTickGap={40} />
        <YAxis {...axis} width={48} tickFormatter={(v) => `${v}${unit}`} />
        <ReferenceLine y={0} stroke="var(--text-muted)" />
        <Tooltip
          cursor={{ fill: "var(--grid)" }}
          content={({ active, payload }) => {
            const d = active ? (payload?.[0]?.payload as (typeof data)[number] | undefined) : undefined;
            return d ? (
              <div className="tooltip">
                <div>{d.title}</div>
                <div className="row"><span>{typeNames[d.type]}</span><b>{d.v > 0 ? "+" : ""}{d.v.toFixed(unit === "bp" ? 1 : 2)}{unit}</b></div>
              </div>
            ) : null;
          }}
        />
        <Bar dataKey="v" radius={4} isAnimationActive={false}>
          {data.map((d, i) => <Cell key={i} fill={SLOTS[typeIndex[d.type] % SLOTS.length]} />)}
        </Bar>
      </BarChart>
    </ResponsiveContainer>
  );
}

export function TypeLegend({ types }: { types: { id: string; name: string }[] }) {
  return (
    <div className="filters">
      {types.map((t, i) => (
        <span key={t.id} className="muted"><span className="swatch" style={{ background: SLOTS[i % SLOTS.length] }} />{t.name}</span>
      ))}
    </div>
  );
}
