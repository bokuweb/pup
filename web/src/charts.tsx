import {
  Area, Bar, BarChart, CartesianGrid, Cell, ComposedChart, Legend, Line, LineChart,
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
  us10y: 0, vix: 0, usdjpy: 0, dxy: 1, gold: 3, wti: 1, btc: 2,
};

export function colorOf(id: string, i: number): string {
  return SLOTS[(MARKET_SLOT[id] ?? i) % SLOTS.length];
}

const axis = { stroke: "var(--text-muted)", fontSize: 11, tickLine: false };

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
  const data = rows(series);
  const names = Object.fromEntries(series.map((s) => [s.id, s.name]));
  const last = (id: string) => data.reduce((acc, r, i) => (r[id] !== undefined ? i : acc), -1);
  return (
    <ResponsiveContainer width="100%" height={height}>
      <LineChart data={data} margin={{ top: 8, right: 76, bottom: 4, left: 0 }}>
        <CartesianGrid stroke="var(--grid)" vertical={false} />
        <XAxis dataKey="k" type="number" domain={["dataMin", "dataMax"]} {...axis} ticks={[-20, -10, -5, 0, 5, 10, 20]} />
        <YAxis {...axis} width={48} tickFormatter={(v) => `${v}${unit}`} />
        <ReferenceLine x={0} stroke="var(--text-muted)" strokeDasharray="3 3" label={{ value: "発表", position: "insideTopLeft", fill: "var(--text-muted)", fontSize: 11 }} />
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

/** Mean path with the interquartile band, for one market and one sub-group. */
export function BandChart({ path, unit, name, height = 260 }: { path: [number, number, number, number][]; unit: string; name: string; height?: number }) {
  const data = path.map(([k, mean, p25, p75]) => ({ k, mean, band: [p25, p75] }));
  return (
    <ResponsiveContainer width="100%" height={height}>
      <ComposedChart data={data} margin={{ top: 8, right: 16, bottom: 4, left: 0 }}>
        <CartesianGrid stroke="var(--grid)" vertical={false} />
        <XAxis dataKey="k" type="number" domain={["dataMin", "dataMax"]} {...axis} ticks={[-20, -10, -5, 0, 5, 10, 20]} />
        <YAxis {...axis} width={48} tickFormatter={(v) => `${v}${unit}`} />
        <ReferenceLine x={0} stroke="var(--text-muted)" strokeDasharray="3 3" />
        <ReferenceLine y={0} stroke="var(--text-muted)" />
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
        <Line dataKey="mean" name={name} stroke="var(--series-1)" strokeWidth={2} dot={false} isAnimationActive={false} />
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
        <Bar dataKey="v" radius={[4, 4, 0, 0]} isAnimationActive={false}>
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
