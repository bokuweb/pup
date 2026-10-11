import { useEffect, useState } from "react";
import type { Meta } from "./api";
import { EventView, Home, MarketView, TypeView, WikiHome, WikiPageView, useJson } from "./views";

function useHash() {
  const [h, setH] = useState(window.location.hash || "#/");
  useEffect(() => {
    const f = () => {
      setH(window.location.hash || "#/");
      window.scrollTo(0, 0);
    };
    window.addEventListener("hashchange", f);
    return () => window.removeEventListener("hashchange", f);
  }, []);
  return h.replace(/^#/, "");
}

export function App() {
  const meta = useJson<Meta>("data/meta.json");
  const route = useHash();
  const ready = meta && !(meta instanceof Error);
  const [, head, ...rest] = route.split("/");
  const arg = decodeURIComponent(rest.join("/"));
  let view = <div className="empty">{meta instanceof Error ? `読み込めませんでした: ${meta.message}` : "読み込み中…"}</div>;
  if (ready) {
    if (head === "event") view = <EventView id={arg} meta={meta} />;
    else if (head === "type") view = <TypeView kind={arg} meta={meta} />;
    else if (head === "market") view = <MarketView id={arg} meta={meta} />;
    else if (head === "wiki" && arg) view = <WikiPageView path={arg} meta={meta} />;
    else if (head === "wiki") view = <WikiHome />;
    else view = <Home meta={meta} />;
  }
  const active = (h: string, a?: string) => (head === h && (a === undefined || arg === a) ? "active" : "");
  return (
    <>
      <header className="top">
        <div className="inner">
          <a className="brand" href="#/"><span className="logo">▲</span>pup</a>
          <nav className="main">
            <a className={head === "" || head === undefined ? "active" : ""} href="#/">概要</a>
            {ready && meta.event_types.map((t) => <a key={t.id} className={active("type", t.id)} href={`#/type/${t.id}`}>{t.name}</a>)}
            {ready && meta.markets.filter((m) => m.wiki).map((m) => <a key={m.id} className={active("market", m.id)} href={`#/market/${m.id}`}>{m.name}</a>)}
            <a className={active("wiki")} href="#/wiki">wiki</a>
          </nav>
        </div>
      </header>
      <main>{view}</main>
      <footer>
        価格データは Yahoo Finance、FOMC は FRB、日銀会合は日本銀行、ECB 理事会は ECB、BoE 金融政策委員会はイングランド銀行、CPI・雇用統計は BLS / FRED より。公開しているのは変化率などの派生値のみ。投資助言ではない。
        {ready && <> · データ更新 {meta.generated_at.slice(0, 10)}</>}
      </footer>
    </>
  );
}
