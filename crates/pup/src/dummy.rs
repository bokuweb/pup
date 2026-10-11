//! Synthetic inputs and template write-ups for trying the whole pipeline offline:
//! `pup dummy` writes random prices and events (all four types) in the `collect` format, and
//! `pup wiki-dummy-write` fills the planned wiki pages with template prose built from the facts,
//! so `wiki-check`, the export and the app can be exercised without network access or LLM cost.
//! Everything it produces is labelled as dummy.

use crate::collect::write_json;
use crate::config::{Measure, MARKETS};
use crate::model::{Bar, Event, Fact, Series, SourceRef};
use crate::wiki::{plan::Plan, Page, LLM_BEGIN, LLM_END};
use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Weekday};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value = "data-dummy")]
    pub data: PathBuf,
    #[arg(long, default_value = "2021-01-01")]
    pub since: NaiveDate,
    #[arg(long, default_value_t = 7)]
    pub seed: u64,
}

/// xorshift64* with Box-Muller normals; no extra dependency for a demo.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn normal(&mut self) -> f64 {
        let (u, v) = (self.next().max(1e-12), self.next());
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

fn nth_weekday(y: i32, m: u32, wd: Weekday, n: u32) -> NaiveDate {
    NaiveDate::from_weekday_of_month_opt(y, m, wd, n as u8).unwrap()
}

fn at(date: NaiveDate, tz: &str, h: u32, m: u32) -> chrono::DateTime<chrono::Utc> {
    let tz: chrono_tz::Tz = tz.parse().unwrap();
    tz.from_local_datetime(&date.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap()))
        .earliest()
        .unwrap()
        .with_timezone(&chrono::Utc)
}

fn event(
    kind: &str,
    date: NaiveDate,
    time: chrono::DateTime<chrono::Utc>,
    approx: bool,
    title: String,
    facts: Vec<Fact>,
) -> Event {
    Event {
        id: format!("{kind}-{date}"),
        kind: kind.into(),
        title: format!("【ダミー】{title}"),
        date,
        time_utc: time,
        time_approx: approx,
        facts,
        sources: vec![SourceRef {
            label: "ダミーデータ（乱数で生成）".into(),
            url: "https://github.com/bokuweb/pup".into(),
        }],
    }
}

pub fn run(a: &Args) -> Result<()> {
    let mut rng = Rng(a.seed.max(1) * 0x9E3779B97F4A7C15);
    let today = chrono::Utc::now().date_naive();
    let start = a.since - Duration::days(420);

    // Events first, so price paths can react to them.
    let mut events = Vec::new();
    let mut rate = 4.25_f64;
    let mut cpi = 3.2_f64;
    let mut ur = 4.0_f64;
    for y in a.since.year()..=today.year() {
        for (m, n) in [
            (1, 4),
            (3, 3),
            (5, 1),
            (6, 3),
            (7, 4),
            (9, 3),
            (10, 5),
            (12, 2),
        ] {
            let d = NaiveDate::from_weekday_of_month_opt(y, m, Weekday::Wed, n)
                .unwrap_or_else(|| nth_weekday(y, m, Weekday::Wed, 4));
            let u = rng.next();
            let (dec, ch) = if u < 0.25 {
                ("利下げ", -0.25)
            } else if u > 0.85 {
                ("利上げ", 0.25)
            } else {
                ("据え置き", 0.0)
            };
            let prev = rate;
            rate = (rate + ch).max(0.25);
            events.push(event(
                "fomc",
                d,
                at(d, "America/New_York", 14, 0),
                false,
                format!("FOMC {}（{dec}）", d.format("%Y年%-m月%-d日")),
                vec![
                    Fact::text("decision", "決定", dec),
                    Fact::text(
                        "target_range",
                        "政策金利（FF 金利誘導目標）",
                        format!("{:.2}〜{:.2}%", rate - 0.25, rate),
                    ),
                    Fact::signed("change_bp", "変更幅", (rate - prev) * 100.0, 0, "bp"),
                    Fact::text(
                        "prev_range",
                        "変更前",
                        format!("{:.2}〜{:.2}%", prev - 0.25, prev),
                    ),
                ],
            ));
        }
        for m in [1, 3, 4, 6, 7, 9, 10, 12] {
            let d = nth_weekday(y, m, Weekday::Fri, 3);
            events.push(event(
                "boj",
                d,
                at(d, "Asia/Tokyo", 12, 0),
                true,
                format!("日銀金融政策決定会合 {}", d.format("%Y年%-m月%-d日")),
                vec![Fact::text("meeting_end", "会合最終日", d.to_string())],
            ));
        }
        for m in 1..=12 {
            let d = nth_weekday(y, m, Weekday::Wed, 2);
            let (pc, pu) = (cpi, ur);
            cpi = (cpi + 0.15 * rng.normal()).clamp(1.0, 6.0);
            ur = (ur + 0.1 * rng.normal()).clamp(3.3, 5.5);
            let label = format!("{}月分", if m == 1 { 12 } else { m - 1 });
            events.push(event(
                "cpi",
                d,
                at(d, "America/New_York", 8, 30),
                false,
                format!("米CPI {}（{label}）", d.format("%Y年%-m月%-d日")),
                vec![
                    Fact::text("reference", "対象月", label.clone()),
                    Fact::num("cpi_yoy", "CPI 前年比", cpi, 1, "%"),
                    Fact::num("core_yoy", "コア CPI 前年比", cpi - 0.3, 1, "%"),
                    Fact::num("cpi_yoy_prev", "CPI 前年比（前月）", pc, 1, "%"),
                    Fact::num("core_yoy_prev", "コア CPI 前年比（前月）", pc - 0.3, 1, "%"),
                ],
            ));
            let d = nth_weekday(y, m, Weekday::Fri, 1);
            events.push(event(
                "nfp",
                d,
                at(d, "America/New_York", 8, 30),
                false,
                format!("米雇用統計 {}（{label}）", d.format("%Y年%-m月%-d日")),
                vec![
                    Fact::text("reference", "対象月", label),
                    Fact::signed(
                        "payrolls_change",
                        "非農業部門雇用者数（前月差、千人）",
                        (150.0 + 90.0 * rng.normal()).round(),
                        0,
                        "千人",
                    ),
                    Fact::num("unemployment", "失業率", ur, 1, "%"),
                    Fact::num("unemployment_prev", "失業率（前月）", pu, 1, "%"),
                ],
            ));
        }
    }
    events.retain(|e| e.date >= a.since && e.time_utc < chrono::Utc::now());
    events.sort_by_key(|e| e.time_utc);

    // Prices: random walks on weekdays, with a shock on event days so the charts have something to show.
    std::fs::create_dir_all(a.data.join("prices"))?;
    for (i, def) in MARKETS.iter().enumerate() {
        let (mut level, vol, drift) = match def.measure {
            Measure::Points => (18.0, 1.2, 0.0),
            Measure::BasisPoints => (3.5, 0.05, 0.0),
            Measure::Percent => (
                1000.0 * (1.0 + i as f64),
                0.011 + 0.002 * (i % 4) as f64,
                0.0003,
            ),
        };
        let mut bars = Vec::new();
        let mut d = start;
        while d <= today {
            if !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
                let shock: f64 = events
                    .iter()
                    .filter(|e| e.date == d)
                    .map(|_| 2.0 * rng.normal())
                    .sum();
                let z = rng.normal() + shock;
                level = match def.measure {
                    Measure::Points => (level + 0.15 * (18.0 - level) + vol * z).max(9.0),
                    Measure::BasisPoints => (level + vol * z).clamp(0.5, 6.0),
                    Measure::Percent => level * (drift + vol * z).exp(),
                };
                bars.push(Bar {
                    date: d,
                    close: level,
                });
            }
            d += Duration::days(1);
        }
        write_json(
            &a.data.join("prices").join(format!("{}.json", def.id)),
            &Series {
                id: def.id.into(),
                symbol: def.symbol.into(),
                bars,
            },
        )?;
    }
    write_json(&a.data.join("events.json"), &events)?;
    tracing::info!(
        "dummy: {} events, {} markets → {}",
        events.len(),
        MARKETS.len(),
        a.data.display()
    );
    Ok(())
}

// ── Template write-ups ───────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct WriteArgs {
    #[arg(long, default_value = "wiki")]
    pub wiki: PathBuf,
    #[arg(long, default_value = ".wiki-work")]
    pub work: PathBuf,
}

struct Notes {
    page_tag: String,
    lines: Vec<String>,
}

impl Notes {
    /// `+1.15%[^tag-3]` with the footnote line recorded.
    fn cite(&mut self, scope: &str, facts: &[Value], key: &str) -> Option<String> {
        let f = facts.iter().find(|f| f["key"] == key)?;
        let text = f["text"].as_str()?;
        let label = format!("{}-{}", self.page_tag, self.lines.len() + 1);
        self.lines
            .push(format!("[^{label}]: fact:{scope}/{key} {text}"));
        Some(format!("{text}[^{label}]"))
    }
}

fn market_facts(bundle: &Value, id: &str) -> Vec<Value> {
    bundle["markets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["market"] == id)
        .and_then(|m| m["facts"].as_array().cloned())
        .unwrap_or_default()
}

fn set_llm(page: &mut Page, text: &str) {
    let (a, b) = (page.body.find(LLM_BEGIN), page.body.find(LLM_END));
    if let (Some(a), Some(b)) = (a, b) {
        page.body.replace_range(
            a..b + LLM_END.len(),
            &format!("{LLM_BEGIN}\n{}\n{LLM_END}", text.trim()),
        );
    }
}

/// Inserts `entry` at the top of the `### 記録` section of a page's LLM block, keeping older entries.
fn prepend_record(page: &mut Page, summary: &str, entry: &str, notes: &[String]) {
    let current = page.llm();
    let (head, records) = match current.split_once("### 記録") {
        Some((h, r)) => (h.trim().to_string(), r.trim().to_string()),
        None => (format!("### 傾向のまとめ\n{summary}"), String::new()),
    };
    let (body, old_notes): (Vec<&str>, Vec<&str>) = records
        .lines()
        .partition(|l| !l.trim_start().starts_with("[^"));
    let head = if head.is_empty() {
        format!("### 傾向のまとめ\n{summary}")
    } else {
        head
    };
    let text = format!(
        "{head}\n\n### 記録\n{entry}\n{}\n\n{}\n{}",
        body.join("\n").trim(),
        notes.join("\n"),
        old_notes.join("\n")
    );
    set_llm(page, &text);
}

pub fn write(a: &WriteArgs) -> Result<()> {
    let plan: Plan = serde_json::from_slice(
        &std::fs::read(a.work.join("plan.json")).context("run wiki-plan first")?,
    )?;
    for t in &plan.tasks {
        let b: Value = serde_json::from_slice(&std::fs::read(&t.bundle)?)?;
        let id = t.event_id.as_str();
        let tscope = format!("type-{}", t.kind);
        let ev = b["event_facts"].as_array().cloned().unwrap_or_default();
        let tf = b["type_facts"].as_array().cloned().unwrap_or_default();
        let (spx, n225, us10y, vix, usdjpy) = (
            market_facts(&b, "spx"),
            market_facts(&b, "n225"),
            market_facts(&b, "us10y"),
            market_facts(&b, "vix"),
            market_facts(&b, "usdjpy"),
        );

        let mut n = Notes {
            page_tag: "e".into(),
            lines: vec![],
        };
        let mut s = String::from("（ダミー文章: 事実データから機械的に組み立てた例。実際の分析ではない）\n\n### 発表前の状態\n");
        if let (Some(a), Some(v)) = (
            n.cite(id, &spx, "spx.pre_ret20"),
            n.cite(id, &vix, "vix.pre_level"),
        ) {
            s.push_str(&format!(
                "- S&P 500 は直前20営業日で {a}、VIX は {v} だった。\n"
            ));
        }
        if let Some(x) = n.cite(id, &us10y, "us10y.pre_ret20") {
            s.push_str(&format!(
                "- 米10年金利の直前20営業日の変化は {x} だった。\n"
            ));
        }
        s.push_str("\n### 発表と反応\n");
        for f in &ev {
            let key = f["key"].as_str().unwrap_or_default();
            if f["text"]
                .as_str()
                .is_some_and(|t| t.contains('%') || t.contains("bp") || t.contains("千人"))
            {
                if let Some(x) = n.cite(id, &ev, key) {
                    s.push_str(&format!("- {}: {x}\n", f["label"].as_str().unwrap_or(key)));
                }
            } else if key != "meeting_end" {
                s.push_str(&format!(
                    "- {}: {}\n",
                    f["label"].as_str().unwrap_or(key),
                    f["text"].as_str().unwrap_or("")
                ));
            }
        }
        if let (Some(a), Some(b2), Some(c), Some(d)) = (
            n.cite(id, &spx, "spx.r1"),
            n.cite(id, &n225, "n225.r1"),
            n.cite(id, &us10y, "us10y.r1"),
            n.cite(id, &usdjpy, "usdjpy.r1"),
        ) {
            s.push_str(&format!("- 発表後1本目の引けまでに S&P 500 は {a}、日経平均は {b2}、米10年金利は {c}、ドル円は {d} だった。\n"));
        }
        s.push_str("\n### その後\n");
        if let (Some(a), Some(b2)) = (n.cite(id, &spx, "spx.r5"), n.cite(id, &n225, "n225.r5")) {
            s.push_str(&format!(
                "- 5本目の引けまでに S&P 500 は {a}、日経平均は {b2} だった。\n"
            ));
        }
        if let Some(x) = n.cite(id, &spx, "spx.r20") {
            s.push_str(&format!("- 20本目の引けまでの S&P 500 は {x} だった。\n"));
        }
        let all_key = format!("{}.all.spx.r1.mean", t.kind);
        if let Some(x) = n.cite(&tscope, &tf, &all_key) {
            s.push_str(&format!("\n### 過去の同種イベントとの比較\n- この種類のイベント全体では、S&P 500 の1本目の平均は {x} である。\n"));
        }
        s.push('\n');
        s.push_str(&n.lines.join("\n"));
        let ep = a.wiki.join(&t.event_page);
        let mut page = Page::read(&ep)?;
        set_llm(&mut page, &s);
        page.frontmatter.retain(|(k, _)| k != "description");
        page.frontmatter.insert(
            2,
            (
                "description".into(),
                serde_json::json!("ダミーの分析（事実データから自動生成した例文）"),
            ),
        );
        page.write(&ep)?;

        let link_from = |from: &str| crate::wiki::link(from, &t.event_page);
        // Event-type page.
        let tp = a.wiki.join(&t.type_page);
        let mut page = Page::read(&tp)?;
        let tag = id.replace('-', "");
        let mut n = Notes {
            page_tag: format!("t{tag}"),
            lines: vec![],
        };
        let summary_note = Notes {
            page_tag: format!("s{tag}"),
            lines: vec![],
        };
        let mut sn = summary_note;
        let summary = sn
            .cite(&tscope, &tf, &all_key)
            .map(|x| {
                format!("- （ダミー）この種類のイベントでは、S&P 500 の1本目の平均は {x} である。")
            })
            .unwrap_or_default();
        let r1 = n.cite(id, &spx, "spx.r1").unwrap_or_default();
        let entry = format!(
            "#### {} [{}]({})\n- （ダミー）S&P 500 の1本目は {r1}。\n",
            t.date,
            t.title,
            link_from(&t.type_page)
        );
        let mut notes = n.lines.clone();
        if page.llm().trim().is_empty() {
            notes.extend(sn.lines);
        }
        prepend_record(&mut page, &summary, &entry, &notes);
        page.write(&tp)?;

        // Market pages: one line each.
        for mp in &t.market_pages {
            let mid = Path::new(mp)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let mf = market_facts(&b, &mid);
            let mut n = Notes {
                page_tag: format!("m{tag}"),
                lines: vec![],
            };
            let (Some(r1), Some(r5)) = (
                n.cite(id, &mf, &format!("{mid}.r1")),
                n.cite(id, &mf, &format!("{mid}.r5")),
            ) else {
                continue;
            };
            let path = a.wiki.join(mp);
            let mut page = Page::read(&path)?;
            let entry = format!(
                "- {} [{}]({}): 1本目 {r1}、5本目 {r5}",
                t.date,
                t.title,
                link_from(mp)
            );
            prepend_record(
                &mut page,
                "- （ダミー）イベントごとの反応の記録。",
                &entry,
                &n.lines,
            );
            page.write(&path)?;
        }
    }
    tracing::info!("wiki-dummy-write: {} tasks", plan.tasks.len());
    Ok(())
}
