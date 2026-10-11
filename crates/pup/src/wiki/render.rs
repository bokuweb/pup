//! Machine-written Markdown: fact tables on event, event-type and market pages, plus index and log.

use crate::config::{self, MARKETS};
use serde_json::Value;

fn fact_text(facts: &Value, key: &str) -> String {
    facts
        .as_array()
        .into_iter()
        .flatten()
        .find(|f| f["key"] == key)
        .and_then(|f| f["text"].as_str())
        .unwrap_or("–")
        .to_string()
}

/// Facts block of an event page.
pub fn event_block(ev: &Value) -> String {
    let e = &ev["event"];
    let mut s = String::from("## 事実\n\n");
    let when = e["time_utc"].as_str().unwrap_or_default();
    s.push_str(&format!(
        "- 発表: {}（UTC {}{}）\n",
        e["date"].as_str().unwrap_or_default(),
        when.get(11..16).unwrap_or(""),
        if e["time_approx"].as_bool() == Some(true) {
            "、時刻は目安"
        } else {
            ""
        }
    ));
    for f in e["facts"].as_array().into_iter().flatten() {
        if f["key"] == "meeting_end" {
            continue;
        }
        s.push_str(&format!(
            "- {}: {}\n",
            f["label"].as_str().unwrap_or_default(),
            f["text"].as_str().unwrap_or_default()
        ));
    }
    for src in e["sources"].as_array().into_iter().flatten() {
        s.push_str(&format!(
            "- 出典: [{}]({})\n",
            src["label"].as_str().unwrap_or_default(),
            src["url"].as_str().unwrap_or_default()
        ));
    }
    let markets: Vec<&Value> = ev["markets"].as_array().into_iter().flatten().collect();
    s.push_str("\n### 発表前の状態\n\n| 市場 | 直前5営業日 | 直前20営業日 | 変動（直前20営業日） | 52週高値から | 200日線から | 水準 |\n|---|---:|---:|---:|---:|---:|---:|\n");
    for m in &markets {
        let id = m["market"].as_str().unwrap_or_default();
        let Some(def) = config::market(id) else {
            continue;
        };
        let f = &m["facts"];
        let k = |x: &str| fact_text(f, &format!("{id}.{x}"));
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            def.name,
            k("pre_ret5"),
            k("pre_ret20"),
            k("pre_vol20"),
            k("pre_dd252"),
            k("pre_ma200"),
            k("pre_level")
        ));
    }
    s.push_str("\n### 発表後の変化（発表前の最後の引けから、発表後 N 本目の引けまで）\n\n| 市場 | 1本目 | 2本目 | 5本目 | 10本目 | 20本目 |\n|---|---:|---:|---:|---:|---:|\n");
    for m in &markets {
        let id = m["market"].as_str().unwrap_or_default();
        let Some(def) = config::market(id) else {
            continue;
        };
        let f = &m["facts"];
        let k = |h: usize| fact_text(f, &format!("{id}.r{h}"));
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            def.name,
            k(1),
            k(2),
            k(5),
            k(10),
            k(20)
        ));
    }
    s.push_str("\n株価指数・為替・商品は変化率、VIX はポイント差、米10年金利は bp 差。チャートはアプリのイベント画面に表示される。\n");
    s
}

/// Aggregates block of an event-type page.
pub fn type_block(t: &Value, events_with_pages: &[(String, String, String)]) -> String {
    let mut s = format!("## 集計（{} 件）\n\n", t["n_events"]);
    s.push_str(
        "発表前の最後の引けから発表後 N 本目の引けまでの変化の平均（括弧内は上昇した割合）。\n\n",
    );
    for g in t["groups"].as_array().into_iter().flatten() {
        s.push_str(&format!(
            "### {}（{} 件）\n\n| 市場 | 1本目 | 5本目 | 20本目 |\n|---|---:|---:|---:|\n",
            g["label"].as_str().unwrap_or_default(),
            g["n_events"]
        ));
        for def in MARKETS.iter().filter(|m| m.wiki) {
            let st = &g["stats"][def.id];
            if st.is_null() {
                continue;
            }
            let cell = |h: &str| {
                let x = &st[h];
                if x.is_null() {
                    return "–".to_string();
                }
                let (d, u) = (def.measure.decimals(), def.measure.unit());
                format!(
                    "{}（{:.0}%）",
                    crate::model::fmt_signed(x["mean"].as_f64().unwrap_or_default(), d, u, true),
                    x["up_share"].as_f64().unwrap_or_default()
                )
            };
            s.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                def.name,
                cell("1"),
                cell("5"),
                cell("20")
            ));
        }
        s.push('\n');
    }
    if !events_with_pages.is_empty() {
        s.push_str("### この種類のイベント（wiki に分析があるもの）\n\n");
        for (date, title, link) in events_with_pages {
            s.push_str(&format!("- {date} [{title}]({link})\n"));
        }
    }
    s
}

/// Reaction table of a market page (most recent first).
pub fn market_block(m: &Value, links: &dyn Fn(&str) -> Option<String>, limit: usize) -> String {
    let def = config::market(m["market"]["id"].as_str().unwrap_or_default());
    let (d, u) = def
        .map(|d| (d.measure.decimals(), d.measure.unit()))
        .unwrap_or((2, "%"));
    let mut s = String::from("## イベント後の変化（新しい順）\n\n| 日付 | イベント | 1本目 | 5本目 | 20本目 |\n|---|---|---:|---:|---:|\n");
    for r in m["reactions"].as_array().into_iter().flatten().take(limit) {
        let id = r["event"].as_str().unwrap_or_default();
        let title = r["title"].as_str().unwrap_or_default();
        let name = match links(id) {
            Some(l) => format!("[{title}]({l})"),
            None => title.to_string(),
        };
        let c = |h: &str| {
            r["post"][h]
                .as_f64()
                .map(|v| crate::model::fmt_signed(v, d, u, true))
                .unwrap_or_else(|| "–".into())
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            r["date"].as_str().unwrap_or_default(),
            name,
            c("1"),
            c("5"),
            c("20")
        ));
    }
    s
}
