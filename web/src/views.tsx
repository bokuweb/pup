import { useEffect, useMemo, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import ForceGraph2D from "react-force-graph-2d";
import { BandChart, PathChart, ReactionBars, SLOTS, TypeLegend } from "./charts";
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
  if (v instanceof Error) return <p className="muted">読み込めませんでした: {v.message}</p>;
  return <p className="muted">読み込み中…</p>;
}

const PANELS: { title: string; markets: string[] }[] = [
  { title: "主要株価指数", markets: ["spx", "n225", "dax", "hsi"] },
  { title: "米国株", markets: ["spx", "ndx", "rut"] },
  { title: "日本株", markets: ["n225", "topix"] },
  { title: "欧州株", markets: ["dax", "sx5e", "ftse"] },
  { title: "米10年金利", markets: ["us10y"] },
  { title: "VIX", markets: ["vix"] },
  { title: "為替", markets: ["usdjpy", "dxy"] },
  { title: "商品・暗号資産", markets: ["gold", "wti", "btc"] },
];

const KEY_MARKETS = ["spx", "n225", "us10y", "usdjpy", "vix"];

/** Wiki index (path → entry), shared by the views that link to analysis. */
function useWikiIndex() {
  const idx = useJson<WikiEntry[]>("wiki/index.json");
  return useMemo(() => (Array.isArray(idx) ? idx : []), [idx]);
}

export function Home({ meta }: { meta: Meta }) {
  const events = useJson<EventSummary[]>("data/events.json");
  const wiki = useWikiIndex();
  const [type, setType] = useState<string>("all");
  if (!Array.isArray(events)) return <Loading v={events} />;
  const analysed = new Set(wiki.filter((w) => w.type === "event").map((w) => w.event_id));
  const shown = events.filter((e) => type === "all" || e.type === type).slice(0, 120);
  const mk = (id: string) => meta.markets.find((m) => m.id === id)!;
  return (
    <>
      <h1>経済イベントと市場の反応</h1>
      <p className="muted">FOMC・日銀会合・米CPI・米雇用統計の前後で、主要な指数がどういう状態からどう動いたか。数値は発表前の最後の引けから発表後1本目の引けまで。{meta.n_events} 件。データ更新 {meta.generated_at.slice(0, 10)}。</p>
      <div className="filters">
        {[{ id: "all", name: "すべて" }, ...meta.event_types].map((t) => (
          <button key={t.id} className="chip" aria-pressed={type === t.id} onClick={() => setType(t.id)}>{t.name}</button>
        ))}
      </div>
      <div className="table-wrap">
        <table>
          <thead>
            <tr><th>日付・イベント</th>{KEY_MARKETS.map((m) => <th key={m}>{mk(m).name}</th>)}<th>分析</th></tr>
          </thead>
          <tbody>
            {shown.map((e) => (
              <tr key={e.id}>
                <td><a href={`#/event/${e.id}`}>{e.title}</a></td>
                {KEY_MARKETS.map((m) => <td key={m}>{fmt(e.r1[m], mk(m))}</td>)}
                <td>{analysed.has(e.id) ? <a href={`#/wiki/events/${e.type}/${e.date}`}>読む</a> : <span className="muted">–</span>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

function FactTable({ d, meta }: { d: EventDetail; meta: Meta }) {
  const f = (m: string, k: string) => d.markets.find((w) => w.market === m)?.facts.find((x) => x.key === `${m}.${k}`)?.text ?? "–";
  const ms = meta.markets.filter((m) => d.markets.some((w) => w.market === m.id));
  return (
    <div className="grid2">
      <div className="table-wrap">
        <table>
          <thead><tr><th>発表前</th><th>直前5本</th><th>直前20本</th><th>52週高値から</th><th>200日線から</th></tr></thead>
          <tbody>{ms.map((m) => <tr key={m.id}><td>{m.name}</td><td>{f(m.id, "pre_ret5")}</td><td>{f(m.id, "pre_ret20")}</td><td>{f(m.id, "pre_dd252")}</td><td>{f(m.id, "pre_ma200")}</td></tr>)}</tbody>
        </table>
      </div>
      <div className="table-wrap">
        <table>
          <thead><tr><th>発表後</th><th>1本目</th><th>2本目</th><th>5本目</th><th>10本目</th><th>20本目</th></tr></thead>
          <tbody>{ms.map((m) => <tr key={m.id}><td>{m.name}</td>{[1, 2, 5, 10, 20].map((h) => <td key={h}>{f(m.id, `r${h}`)}</td>)}</tr>)}</tbody>
        </table>
      </div>
    </div>
  );
}

export function EventView({ id, meta }: { id: string; meta: Meta }) {
  const d = useJson<EventDetail>(`data/events/${id}.json`);
  const wiki = useWikiIndex();
  if (!d || d instanceof Error) return <Loading v={d} />;
  const e = d.event;
  const page = wiki.find((w) => w.event_id === id);
  const series = (ids: string[]) =>
    ids.flatMap((mid) => {
      const w = d.markets.find((x) => x.market === mid);
      const m = meta.markets.find((x) => x.id === mid);
      return w && m ? [{ id: mid, name: m.name, points: w.path }] : [];
    });
  return (
    <>
      <h1>{e.title}</h1>
      <p className="muted">
        発表 {e.date}（UTC {e.time_utc.slice(11, 16)}{e.time_approx ? "、時刻は目安" : ""}）・
        <a href={`#/type/${e.type}`}>{meta.event_types.find((t) => t.id === e.type)?.name} の傾向</a>
        {page && <> ・<a href={`#/wiki/${page.path}`}>wiki の分析を読む</a></>}
      </p>
      <div className="card">
        {e.facts.filter((f) => f.key !== "meeting_end").map((f) => <div key={f.key}><span className="muted">{f.label}:</span> <b>{f.text}</b></div>)}
        {e.sources.map((s) => <div key={s.url} className="muted">出典: <a href={s.url}>{s.label}</a></div>)}
      </div>
      <h2>前後20営業日の推移（発表前の最後の引けからの変化）</h2>
      <div className="grid2">
        {PANELS.map((p) => {
          const s = series(p.markets);
          if (!s.length) return null;
          const m = meta.markets.find((x) => x.id === p.markets[0])!;
          return (
            <div className="card" key={p.title}>
              <div className="chart-title">{p.title}（{unit(m)}）</div>
              <PathChart series={s} unit={unit(m)} />
            </div>
          );
        })}
      </div>
      <h2>事実（表）</h2>
      <FactTable d={d} meta={meta} />
    </>
  );
}

export function TypeView({ kind, meta }: { kind: string; meta: Meta }) {
  const t = useJson<TypeDetail>(`data/types/${kind}.json`);
  const wiki = useWikiIndex();
  const [group, setGroup] = useState("all");
  const [market, setMarket] = useState("spx");
  if (!t || t instanceof Error) return <Loading v={t} />;
  const g = t.groups.find((x) => x.id === group) ?? t.groups[0];
  const m = meta.markets.find((x) => x.id === market)!;
  const page = wiki.find((w) => w.type === "event-type" && w.event_type === kind);
  return (
    <>
      <h1>{t.type.name}</h1>
      <p className="muted">{t.type.description}・{t.n_events} 件{page && <> ・<a href={`#/wiki/${page.path}`}>wiki の傾向と記録</a></>}</p>
      <div className="filters">
        {t.groups.map((x) => <button key={x.id} className="chip" aria-pressed={g.id === x.id} onClick={() => setGroup(x.id)}>{x.label}（{x.n_events}）</button>)}
      </div>
      <div className="filters">
        {meta.markets.map((x) => <button key={x.id} className="chip" aria-pressed={market === x.id} onClick={() => setMarket(x.id)}>{x.name}</button>)}
      </div>
      <div className="card">
        <div className="chart-title">{m.name}：{g.label}の平均の推移と四分位の幅（{unit(m)}）</div>
        {g.path[market] ? <BandChart path={g.path[market]} unit={unit(m)} name={m.name} /> : <p className="muted">データなし</p>}
      </div>
      <h2>発表後の変化の統計（{g.label}）</h2>
      <div className="table-wrap">
        <table>
          <thead><tr><th>市場</th>{meta.horizons.map((h) => <th key={h}>{h}本目 平均</th>)}<th>1本目 中央値</th><th>1本目 上昇割合</th><th>件数</th></tr></thead>
          <tbody>
            {meta.markets.filter((x) => g.stats[x.id]).map((x) => {
              const s = g.stats[x.id];
              return (
                <tr key={x.id}>
                  <td>{x.name}</td>
                  {meta.horizons.map((h) => <td key={h}>{fmt(s[h]?.mean, x)}</td>)}
                  <td>{fmt(s["1"]?.median, x)}</td>
                  <td>{s["1"] ? `${s["1"].up_share.toFixed(0)}%` : "–"}</td>
                  <td>{s["1"]?.n ?? "–"}</td>
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
  const data = d.reactions.filter((r) => r.post[h] !== undefined).slice(0, 80).reverse().map((r) => ({ title: r.title, date: r.date, type: r.type, v: r.post[h] }));
  return (
    <>
      <h1>{m.name}</h1>
      <p className="muted">{m.symbol}・イベントごとの、発表前の最後の引けから {h} 本目の引けまでの変化（直近80件）</p>
      <div className="filters">
        {meta.horizons.map((x) => <button key={x} className="chip" aria-pressed={h === String(x)} onClick={() => setH(String(x))}>{x}本目</button>)}
      </div>
      <TypeLegend types={meta.event_types} />
      <div className="card"><ReactionBars data={data} unit={unit(m)} typeIndex={typeIndex} typeNames={typeNames} /></div>
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
      <p className="muted">
        <span className="badge">{fm.type}</span>{" "}
        {fm.event_id && <a href={`#/event/${fm.event_id}`}>チャートと事実の表</a>}
        {fm.type === "event-type" && <a href={`#/type/${fm.event_type}`}>チャートで見る</a>}
        {fm.type === "market" && <a href={`#/market/${fm.market}`}>チャートで見る</a>}
      </p>
      {fm.description && <p>{fm.description}</p>}
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
      {fm.event_id && <EventCharts id={fm.event_id} meta={meta} />}
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
    <>
      <h2>チャート：主要株価指数</h2>
      <div className="card"><PathChart series={s} unit="%" /></div>
      <p className="muted"><a href={`#/event/${id}`}>金利・VIX・為替などのチャートを見る</a></p>
    </>
  );
}

const TYPE_ORDER = ["event", "event-type", "market", "topic"];
const TYPE_NAMES: Record<string, string> = { event: "イベント", "event-type": "イベントの種類", market: "市場", topic: "論点" };

export function WikiHome() {
  const idx = useJson<WikiEntry[]>("wiki/index.json");
  const graph = useJson<WikiGraph>("wiki/graph.json");
  const [width, setWidth] = useState(800);
  useEffect(() => {
    const f = () => setWidth(Math.min(1088, window.innerWidth - 32));
    f();
    window.addEventListener("resize", f);
    return () => window.removeEventListener("resize", f);
  }, []);
  if (!Array.isArray(idx)) return <Loading v={idx} />;
  const color = (t: string) => getComputedStyle(document.documentElement).getPropertyValue(`--series-${(TYPE_ORDER.indexOf(t) % 4) + 1}`).trim() || "#888";
  return (
    <>
      <h1>wiki</h1>
      <p className="muted">LLM（Claude）が事実データだけを根拠に書いた分析。数値はすべて機械で事実と照合済み。<a href="#/wiki/index">索引</a>・<a href="#/wiki/log">更新ログ</a></p>
      {graph && !(graph instanceof Error) && graph.nodes.length > 0 && (
        <div className="card">
          <TypeLegend types={TYPE_ORDER.map((t) => ({ id: t, name: TYPE_NAMES[t] }))} />
          <ForceGraph2D
            graphData={graph}
            width={width - 34}
            height={420}
            nodeLabel={(n) => (n as { title: string }).title}
            nodeColor={(n) => color((n as { type: string }).type)}
            nodeRelSize={5}
            linkColor={() => "rgba(128,128,128,0.35)"}
            onNodeClick={(n) => (window.location.hash = `#/wiki/${(n as { id: string }).id}`)}
          />
        </div>
      )}
      {TYPE_ORDER.map((t) => {
        const xs = idx.filter((x) => x.type === t).sort((a, b) => (b.date ?? "").localeCompare(a.date ?? "") || a.title.localeCompare(b.title));
        if (!xs.length) return null;
        return (
          <section key={t}>
            <h2>{TYPE_NAMES[t]}（{xs.length}）</h2>
            <ul>{xs.map((x) => <li key={x.path}><a href={`#/wiki/${x.path}`}>{x.title}</a>{x.description && <span className="muted"> — {x.description}</span>}</li>)}</ul>
          </section>
        );
      })}
    </>
  );
}

export { SLOTS };
