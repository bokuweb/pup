import { useEffect, useMemo, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import ForceGraph2D from "react-force-graph-2d";
import { BandChart, MoveBars, PathChart, ReactionBars, SLOTS, TypeLegend, heat, heatScale, tint } from "./charts";
import type { EventDetail, EventSummary, MarketDetail, Meta, TypeDetail, WikiEntry, WikiGraph, WikiPage } from "./api";
import { fmt, getJson, unit } from "./api";

export function useJson<T>(path: string | null): T | null | Error {
  const [v, setV] = useState<T | null | Error>(null);
  useEffect(() => {
    setV(null);
    if (path) getJson<T>(path).then(setV, (e) => setV(e instanceof Error ? e : new Error(String(e))));
  }, [path]);
  return v;
}

function Loading({ v }: { v: unknown }) {
  if (v instanceof Error) return <div className="empty">読み込めませんでした: {v.message}</div>;
  return <div className="empty">読み込み中…</div>;
}

const PANELS: { title: string; markets: string[] }[] = [
  { title: "主要株価指数", markets: ["spx", "n225", "dax", "hsi"] },
  { title: "米10年金利", markets: ["us10y"] },
  { title: "VIX", markets: ["vix"] },
  { title: "ドル円", markets: ["usdjpy"] },
  { title: "米国株", markets: ["spx", "ndx", "rut"] },
  { title: "日本株", markets: ["n225", "topix"] },
  { title: "欧州株", markets: ["dax", "sx5e", "ftse"] },
  { title: "商品・暗号資産", markets: ["gold", "wti", "btc"] },
];

const KEY_MARKETS = ["spx", "ndx", "n225", "dax", "us10y", "usdjpy", "vix", "gold"];

function useWikiIndex() {
  const idx = useJson<WikiEntry[]>("wiki/index.json");
  return useMemo(() => (Array.isArray(idx) ? idx : []), [idx]);
}

const typeName = (meta: Meta, id: string) => meta.event_types.find((t) => t.id === id)?.name ?? id;

/** Events × key markets, tinted by the first-close move. */
function Heatmap({ events, meta, analysed }: { events: EventSummary[]; meta: Meta; analysed: Set<string> }) {
  const ms = KEY_MARKETS.map((id) => meta.markets.find((m) => m.id === id)!).filter(Boolean);
  const typeIdx = Object.fromEntries(meta.event_types.map((t, i) => [t.id, i]));
  return (
    <div className="card heatmap-card">
      <div className="heatmap-scroll">
        <table className="heatmap">
          <colgroup><col className="ev" />{ms.map((m) => <col key={m.id} />)}</colgroup>
          <thead>
            <tr>
              <th className="ev">イベント</th>
              {ms.map((m) => <th key={m.id}>{m.name.split("（")[0]}<span className="u">{unit(m)}</span></th>)}
            </tr>
          </thead>
          <tbody>
            {events.map((e) => (
              <tr key={e.id} onClick={() => (window.location.hash = `#/event/${e.id}`)}>
                <td className="ev">
                  <span className="dot" style={{ background: SLOTS[typeIdx[e.type] % SLOTS.length] }} />
                  <span className="t">
                    <a href={`#/event/${e.id}`} title={e.title}>{e.title.replace(/【ダミー】/, "").replace(/ \d{4}年\d+月\d+日/, "")}</a>
                    <span className="d">{e.date}{analysed.has(e.id) ? " · 分析あり" : ""}</span>
                  </span>
                </td>
                {ms.map((m) => {
                  const v = e.r1[m.id];
                  return (
                    <td key={m.id} className="v" title={`${m.name} ${fmt(v, m)}`}>
                      <span style={(({ border, background }) => ({ borderColor: border, background }))(tint(v, heatScale(unit(m))))}>{fmt(v, m).replace(/(%|bp|pt)$/, "")}</span>
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function Home({ meta }: { meta: Meta }) {
  const events = useJson<EventSummary[]>("data/events.json");
  const wiki = useWikiIndex();
  const [type, setType] = useState<string>("all");
  if (!Array.isArray(events)) return <Loading v={events} />;
  const analysed = new Set(wiki.filter((w) => w.type === "event").map((w) => w.event_id!));
  const latest = events[0];
  const spx = meta.markets.find((m) => m.id === "spx")!;
  const recent = events.slice(0, 40);
  const biggest = [...recent].sort((a, b) => Math.abs(b.r1.spx ?? 0) - Math.abs(a.r1.spx ?? 0))[0];
  const byType = meta.event_types.map((t) => ({ t, n: events.filter((e) => e.type === t.id).length }));
  const shown = events.filter((e) => type === "all" || e.type === type).slice(0, 40);
  const mk = (id: string) => meta.markets.find((m) => m.id === id)!;
  return (
    <>
      <section className="hero">
        <div>
          <div className="eyebrow">Macro events × Markets</div>
          <h1>経済イベントで、<br /><span className="grad">市場はどう動いたか。</span></h1>
          <p className="lead">FOMC・日銀・米CPI・米雇用統計の前後で、主要な指数がどんな状態から、どう動いたか。事実は機械で計算し、分析は Claude が事実だけを根拠に書く。</p>
        </div>
        {latest && (
          <a className="card hover latest" href={`#/event/${latest.id}`}>
            <div className="muted">最新のイベント · {latest.date}</div>
            <div className="title" style={{ marginTop: 4 }}>{latest.title.replace(/【ダミー】/, "")}</div>
            <div className="chips">
              {["spx", "n225", "us10y", "usdjpy"].map((id) => (
                <span className="pill" key={id}>{mk(id).name}<b className="num">{fmt(latest.r1[id], mk(id))}</b></span>
              ))}
            </div>
          </a>
        )}
      </section>

      <div className="grid k4">
        <div className="card kpi"><div className="label">イベント</div><div className="value num">{meta.n_events}</div><div className="foot">{byType.map((x) => `${x.t.name} ${x.n}`).join(" · ")}</div></div>
        <div className="card kpi"><div className="label">wiki の分析</div><div className="value num">{analysed.size}</div><div className="foot">数値はすべて事実と照合済み</div></div>
        {biggest && (
          <a className="card kpi hover" href={`#/event/${biggest.id}`}>
            <div className="label">直近40件で S&P 500 が最も動いた</div>
            <div className="value num">{fmt(biggest.r1.spx, spx)}</div>
            <div className="foot">{biggest.title.replace(/【ダミー】/, "")}</div>
          </a>
        )}
        <div className="card kpi"><div className="label">対象の市場</div><div className="value num">{meta.markets.length}</div><div className="foot">株価指数・金利・為替・VIX・商品・暗号資産</div></div>
      </div>

      <h2>反応のヒートマップ <span className="sub">発表前の最後の引け → 発表後1本目の引け</span></h2>
      <div className="filters" style={{ justifyContent: "space-between", alignItems: "center" }}>
        <div className="filters" style={{ margin: 0 }}>
          {[{ id: "all", name: "すべて" }, ...meta.event_types].map((t) => (
            <button key={t.id} className="chip" aria-pressed={type === t.id} onClick={() => setType(t.id)}>{t.name}</button>
          ))}
        </div>
        <div className="legend-div">下落<span className="bar" />上昇<span>（株・為替 %、金利 bp、VIX pt）</span></div>
      </div>
      <Heatmap events={shown} meta={meta} analysed={analysed} />
    </>
  );
}

function FactTable({ d, meta }: { d: EventDetail; meta: Meta }) {
  const w = (m: string) => d.markets.find((x) => x.market === m);
  const f = (m: string, k: string) => w(m)?.facts.find((x) => x.key === `${m}.${k}`)?.text ?? "–";
  const val = (m: string, k: string) => w(m)?.facts.find((x) => x.key === `${m}.${k}`)?.value;
  const ms = meta.markets.filter((m) => w(m.id));
  return (
    <div className="grid c2">
      <div className="table-wrap">
        <table>
          <thead><tr><th>発表前の状態</th><th>直前5本</th><th>直前20本</th><th>52週高値から</th><th>200日線から</th></tr></thead>
          <tbody>{ms.map((m) => <tr key={m.id}><td>{m.name}</td>{["pre_ret5", "pre_ret20", "pre_dd252", "pre_ma200"].map((k) => <td key={k} className="heat" style={{ borderLeftColor: heat(val(m.id, k), heatScale(unit(m)) * 3) }}>{f(m.id, k)}</td>)}</tr>)}</tbody>
        </table>
      </div>
      <div className="table-wrap">
        <table>
          <thead><tr><th>発表後の変化</th><th>1本目</th><th>2本目</th><th>5本目</th><th>10本目</th><th>20本目</th></tr></thead>
          <tbody>{ms.map((m) => <tr key={m.id}><td>{m.name}</td>{[1, 2, 5, 10, 20].map((h) => <td key={h} className="heat" style={{ background: heat(val(m.id, `r${h}`), heatScale(unit(m)) * Math.sqrt(h)) }}>{f(m.id, `r${h}`)}</td>)}</tr>)}</tbody>
        </table>
      </div>
    </div>
  );
}

export function EventView({ id, meta }: { id: string; meta: Meta }) {
  const d = useJson<EventDetail>(`data/events/${id}.json`);
  const wiki = useWikiIndex();
  const [h, setH] = useState(1);
  if (!d || d instanceof Error) return <Loading v={d} />;
  const e = d.event;
  const page = wiki.find((w) => w.event_id === id);
  const series = (ids: string[]) =>
    ids.flatMap((mid) => {
      const w = d.markets.find((x) => x.market === mid);
      const m = meta.markets.find((x) => x.id === mid);
      return w && m ? [{ id: mid, name: m.name, points: w.path }] : [];
    });
  const moves = (u: string) =>
    meta.markets.filter((m) => unit(m) === u).flatMap((m) => {
      const f = d.markets.find((x) => x.market === m.id)?.facts.find((x) => x.key === `${m.id}.r${h}`);
      return f && f.value !== undefined ? [{ name: m.name, v: f.value, text: f.text, unit: u }] : [];
    });
  const facts = e.facts.filter((f) => f.key !== "meeting_end" && f.key !== "reference");
  return (
    <>
      <div className="eyebrow">{typeName(meta, e.type)} · {e.date}</div>
      <h1>{e.title.replace(/【ダミー】/, "")}</h1>
      <p className="muted" style={{ marginTop: 0 }}>
        発表 UTC {e.time_utc.slice(11, 16)}{e.time_approx ? "（目安）" : ""} ·{" "}
        <a href={`#/type/${e.type}`}>{typeName(meta, e.type)} の傾向</a>
        {page && <> · <a href={`#/wiki/${page.path}`}>wiki の分析を読む →</a></>}
        {e.sources.map((s) => <span key={s.url}> · <a href={s.url}>出典</a></span>)}
      </p>
      {facts.length > 0 && (
        <div className="card facts" style={{ marginTop: 16 }}>
          {facts.map((f) => <div className="fact" key={f.key}><div className="label">{f.label}</div><div className="value num">{f.text}</div></div>)}
        </div>
      )}

      <h2>市場ごとの反応 <span className="sub">発表前の最後の引けから</span></h2>
      <div className="filters">
        {[1, 2, 5, 10, 20].map((x) => <button key={x} className="chip" aria-pressed={h === x} onClick={() => setH(x)}>{x}本目</button>)}
      </div>
      <div className="grid c2">
        <div className="card"><div className="chart-head"><span className="chart-title">株価指数・為替・商品</span><span className="chart-unit">%</span></div><MoveBars data={moves("%")} /></div>
        <div className="grid" style={{ alignContent: "start" }}>
          <div className="card"><div className="chart-head"><span className="chart-title">金利</span><span className="chart-unit">bp</span></div><MoveBars data={moves("bp")} height={70} /></div>
          <div className="card"><div className="chart-head"><span className="chart-title">VIX</span><span className="chart-unit">pt</span></div><MoveBars data={moves("pt")} height={70} /></div>
        </div>
      </div>

      <h2>前後20営業日の推移 <span className="sub">発表前の最後の引けからの変化</span></h2>
      <div className="grid c2">
        {PANELS.map((p) => {
          const s = series(p.markets);
          if (!s.length) return null;
          const m = meta.markets.find((x) => x.id === p.markets[0])!;
          return (
            <div className="card" key={p.title}>
              <div className="chart-head"><span className="chart-title">{p.title}</span><span className="chart-unit">{unit(m)}</span></div>
              <PathChart series={s} unit={unit(m)} />
            </div>
          );
        })}
      </div>
      <h2>事実の表</h2>
      <FactTable d={d} meta={meta} />
    </>
  );
}

/** The equity index and the rate or currency that each event type is mostly about. */
const TYPE_KEYS: Record<string, [string, string]> = {
  boj: ["n225", "usdjpy"],
  jpcpi: ["n225", "usdjpy"],
  tankan: ["n225", "usdjpy"],
  ecb: ["sx5e", "eurusd"],
  boe: ["ftse", "gbpusd"],
};

export function TypeView({ kind, meta }: { kind: string; meta: Meta }) {
  const t = useJson<TypeDetail>(`data/types/${kind}.json`);
  const wiki = useWikiIndex();
  const [eq, rate] = TYPE_KEYS[kind] ?? ["spx", "us10y"];
  const eqDef = meta.markets.find((x) => x.id === eq)!;
  const rateDef = meta.markets.find((x) => x.id === rate)!;
  const [group, setGroup] = useState("all");
  const [market, setMarket] = useState(eq);
  if (!t || t instanceof Error) return <Loading v={t} />;
  const g = t.groups.find((x) => x.id === group) ?? t.groups[0];
  const m = meta.markets.find((x) => x.id === market)!;
  const page = wiki.find((w) => w.type === "event-type" && w.event_type === kind);
  const s1 = g.stats[eq]?.["1"];
  return (
    <>
      <div className="eyebrow">Event type</div>
      <h1>{t.type.name}</h1>
      <p className="lead">{t.type.description}{page && <> · <a href={`#/wiki/${page.path}`}>wiki の傾向と記録 →</a></>}</p>
      <div className="grid k4" style={{ marginTop: 20 }}>
        <div className="card kpi"><div className="label">件数（{g.label}）</div><div className="value num">{g.n_events}</div></div>
        <div className="card kpi"><div className="label">{eqDef.name} 1本目の平均</div><div className="value num">{fmt(s1?.mean, eqDef)}</div></div>
        <div className="card kpi"><div className="label">{eqDef.name} 1本目で上昇した割合</div><div className="value num">{s1 ? `${s1.up_share.toFixed(0)}%` : "–"}</div></div>
        <div className="card kpi"><div className="label">{rateDef.name} 1本目の平均</div><div className="value num">{fmt(g.stats[rate]?.["1"]?.mean, rateDef)}</div></div>
      </div>
      <h2>平均の推移と四分位の幅</h2>
      <div className="filters">
        {t.groups.map((x) => <button key={x.id} className="chip" aria-pressed={g.id === x.id} onClick={() => setGroup(x.id)}>{x.label} <span className="num" style={{ opacity: 0.6 }}>{x.n_events}</span></button>)}
      </div>
      <div className="filters">
        {meta.markets.map((x) => <button key={x.id} className="chip" aria-pressed={market === x.id} onClick={() => setMarket(x.id)}>{x.name}</button>)}
      </div>
      <div className="card">
        <div className="chart-head"><span className="chart-title">{m.name} · {g.label}</span><span className="chart-unit">{unit(m)}（線 = 平均、帯 = 25〜75%）</span></div>
        {g.path[market] ? <BandChart path={g.path[market]} unit={unit(m)} name={m.name} height={320} /> : <div className="empty">データなし</div>}
      </div>
      <h2>発表後の変化の統計 <span className="sub">{g.label} · 平均</span></h2>
      <div className="table-wrap">
        <table>
          <thead><tr><th>市場</th>{meta.horizons.map((h) => <th key={h}>{h}本目</th>)}<th>1本目 中央値</th><th>1本目 上昇割合</th><th>件数</th></tr></thead>
          <tbody>
            {meta.markets.filter((x) => g.stats[x.id]).map((x) => {
              const s = g.stats[x.id];
              return (
                <tr key={x.id}>
                  <td>{x.name}</td>
                  {meta.horizons.map((h) => <td key={h} className="heat" style={{ background: heat(s[h]?.mean, heatScale(unit(x)) * 0.6 * Math.sqrt(h)) }}>{fmt(s[h]?.mean, x)}</td>)}
                  <td className="heat">{fmt(s["1"]?.median, x)}</td>
                  <td className="heat">{s["1"] ? `${s["1"].up_share.toFixed(0)}%` : "–"}</td>
                  <td className="heat">{s["1"]?.n ?? "–"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}

export function MarketView({ id, meta }: { id: string; meta: Meta }) {
  const d = useJson<MarketDetail>(`data/markets/${id}.json`);
  const [h, setH] = useState("1");
  if (!d || d instanceof Error) return <Loading v={d} />;
  const m = d.market;
  const typeIndex = Object.fromEntries(meta.event_types.map((t, i) => [t.id, i]));
  const typeNames = Object.fromEntries(meta.event_types.map((t) => [t.id, t.name]));
  const data = d.reactions.filter((r) => r.post[h] !== undefined).slice(0, 80).reverse().map((r) => ({ title: r.title.replace(/【ダミー】/, ""), date: r.date, type: r.type, v: r.post[h] }));
  const avg = (t: string) => {
    const xs = d.reactions.filter((r) => r.type === t && r.post[h] !== undefined).map((r) => r.post[h]);
    return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : undefined;
  };
  return (
    <>
      <div className="eyebrow">Market · {m.symbol}</div>
      <h1>{m.name}</h1>
      <p className="lead">イベントごとの、発表前の最後の引けから {h} 本目の引けまでの変化。</p>
      <div className="grid k4" style={{ marginTop: 20 }}>
        {meta.event_types.map((t, i) => (
          <a className="card kpi hover" key={t.id} href={`#/type/${t.id}`}>
            <div className="label"><span className="swatch" style={{ background: SLOTS[i] }} />{t.name}の後（平均）</div>
            <div className="value num">{fmt(avg(t.id), m)}</div>
          </a>
        ))}
      </div>
      <h2>イベントごとの反応 <span className="sub">直近80件</span></h2>
      <div className="filters">
        {meta.horizons.map((x) => <button key={x} className="chip" aria-pressed={h === String(x)} onClick={() => setH(String(x))}>{x}本目</button>)}
      </div>
      <div className="card">
        <TypeLegend types={meta.event_types} />
        <ReactionBars data={data} unit={unit(m)} typeIndex={typeIndex} typeNames={typeNames} height={320} />
      </div>
    </>
  );
}

function resolve(from: string, target: string): string {
  const parts = from.split("/");
  parts.pop();
  for (const seg of target.split("/")) {
    if (seg === "..") parts.pop();
    else if (seg && seg !== ".") parts.push(seg);
  }
  return parts.join("/").replace(/\.md$/, "");
}

export function WikiPageView({ path, meta }: { path: string; meta: Meta }) {
  const p = useJson<WikiPage>(`wiki/page/${path}.json`);
  if (!p || p instanceof Error) return <Loading v={p} />;
  const fm = p.frontmatter as Record<string, string>;
  const body = p.body.replace(/<!--[\s\S]*?-->/g, "");
  return (
    <div className="prose">
      <div className="eyebrow">wiki · {fm.type}</div>
      {fm.description && <p className="lead" style={{ marginTop: 6 }}>{fm.description}</p>}
      <p className="muted">
        {fm.event_id && <a href={`#/event/${fm.event_id}`}>チャートと事実の表 →</a>}
        {fm.type === "event-type" && <a href={`#/type/${fm.event_type}`}>チャートで見る →</a>}
        {fm.type === "market" && <a href={`#/market/${fm.market}`}>チャートで見る →</a>}
      </p>
      {fm.event_id && <EventCharts id={fm.event_id} meta={meta} />}
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => {
            const h = href ?? "";
            if (/^https?:/.test(h) || h.startsWith("#")) return <a href={h}>{children}</a>;
            return <a href={`#/wiki/${resolve(p.path + ".md", h)}`}>{children}</a>;
          },
          table: ({ children }) => <div className="table-wrap"><table>{children}</table></div>,
        }}
      >
        {body}
      </Markdown>
    </div>
  );
}

function EventCharts({ id, meta }: { id: string; meta: Meta }) {
  const d = useJson<EventDetail>(`data/events/${id}.json`);
  if (!d || d instanceof Error) return null;
  const s = PANELS[0].markets.flatMap((mid) => {
    const w = d.markets.find((x) => x.market === mid);
    const m = meta.markets.find((x) => x.id === mid);
    return w && m ? [{ id: mid, name: m.name, points: w.path }] : [];
  });
  return (
    <div className="card" style={{ margin: "16px 0 8px" }}>
      <div className="chart-head"><span className="chart-title">主要株価指数 · 前後20営業日</span><span className="chart-unit">%</span></div>
      <PathChart series={s} unit="%" />
    </div>
  );
}

const TYPE_ORDER = ["event", "event-type", "market", "topic"];
const TYPE_NAMES: Record<string, string> = { event: "イベント", "event-type": "イベントの種類", market: "市場", topic: "論点" };

export function WikiHome() {
  const idx = useJson<WikiEntry[]>("wiki/index.json");
  const graph = useJson<WikiGraph>("wiki/graph.json");
  const [width, setWidth] = useState(800);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fg = useRef<any>(null);
  useEffect(() => {
    const f = () => setWidth(Math.min(1200, window.innerWidth - 40));
    f();
    window.addEventListener("resize", f);
    return () => window.removeEventListener("resize", f);
  }, []);
  if (!Array.isArray(idx)) return <Loading v={idx} />;
  const color = (t: string) => getComputedStyle(document.documentElement).getPropertyValue(`--series-${(TYPE_ORDER.indexOf(t) % 4) + 1}`).trim() || "#888";
  return (
    <>
      <div className="eyebrow">Knowledge graph</div>
      <h1>wiki</h1>
      <p className="lead">Claude が事実データだけを根拠に書いた分析。書かれた数値はすべて、脚注の事実と機械で照合してから公開している。<a href="#/wiki/index">索引</a> · <a href="#/wiki/log">更新ログ</a></p>
      {graph && !(graph instanceof Error) && graph.nodes.length > 0 && (
        <div className="card" style={{ marginTop: 20, padding: 8, overflow: "hidden" }}>
          <div style={{ padding: "6px 10px 0" }}><TypeLegend types={TYPE_ORDER.map((t) => ({ id: t, name: TYPE_NAMES[t] }))} /></div>
          <ForceGraph2D
            ref={fg}
            graphData={graph}
            width={width - 18}
            height={460}
            backgroundColor="rgba(0,0,0,0)"
            cooldownTicks={120}
            onEngineStop={() => fg.current?.zoomToFit(400, 40)}
            nodeLabel={(n) => (n as { title: string }).title}
            nodeColor={(n) => color((n as { type: string }).type)}
            nodeRelSize={5}
            linkColor={() => "rgba(140,140,160,0.25)"}
            onNodeClick={(n) => (window.location.hash = `#/wiki/${(n as { id: string }).id}`)}
          />
        </div>
      )}
      <div className="grid c2" style={{ marginTop: 8 }}>
        {TYPE_ORDER.map((t) => {
          const xs = idx.filter((x) => x.type === t).sort((a, b) => (b.date ?? "").localeCompare(a.date ?? "") || a.title.localeCompare(b.title));
          if (!xs.length) return null;
          return (
            <section key={t}>
              <h2>{TYPE_NAMES[t]} <span className="sub">{xs.length}</span></h2>
              <div className="card list" style={{ padding: 6 }}>
                {xs.slice(0, 30).map((x) => (
                  <a className="item" key={x.path} href={`#/wiki/${x.path}`}>
                    <span>{x.title.replace(/【ダミー】/, "")}<div className="desc">{x.description}</div></span>
                    {x.date && <span className="muted num">{x.date}</span>}
                  </a>
                ))}
              </div>
            </section>
          );
        })}
      </div>
    </>
  );
}
