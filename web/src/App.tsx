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
  let view = <p className="muted">{meta instanceof Error ? `読み込めませんでした: ${meta.message}` : "読み込み中…"}</p>;
  if (ready) {
    const [, head, ...rest] = route.split("/");
    const arg = decodeURIComponent(rest.join("/"));
    if (head === "event") view = <EventView id={arg} meta={meta} />;
    else if (head === "type") view = <TypeView kind={arg} meta={meta} />;
    else if (head === "market") view = <MarketView id={arg} meta={meta} />;
    else if (head === "wiki" && arg) view = <WikiPageView path={arg} meta={meta} />;
    else if (head === "wiki") view = <WikiHome />;
    else view = <Home meta={meta} />;
  }
  return (
    <>
      <header className="top">
        <a className="brand" href="#/">pup</a>
        <nav>
          <a href="#/">イベント</a>
          {ready && meta.event_types.map((t) => <a key={t.id} href={`#/type/${t.id}`}>{t.name}</a>)}
          {ready && meta.markets.filter((m) => m.wiki).map((m) => <a key={m.id} href={`#/market/${m.id}`}>{m.name}</a>)}
          <a href="#/wiki">wiki</a>
        </nav>
      </header>
      <main>{view}</main>
    </>
  );
}
