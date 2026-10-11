//! Event windows: the state of each market before an event and how it moved afterwards.
//!
//! For a market and an event at instant `t`, the "pre" bar is the last daily bar whose close is
//! before `t` (so a 08:30 New York release uses the previous US close, and Tokyo's reaction to a
//! 14:00 New York FOMC statement is the next Tokyo close). Offsets count trading sessions of that
//! market: `-k` before the pre bar, `+k` after it.

use crate::config::{MarketDef, Measure, HORIZONS, MARKETS, POST_SESSIONS, PRE_SESSIONS};
use crate::model::{fmt_signed, Event, Fact, Series};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct MarketWindow {
    pub market: String,
    pub facts: Vec<Fact>,
    /// `(offset, change from the pre close)`; offset 0 is the pre close.
    pub path: Vec<(i64, f64)>,
    /// Post-event changes keyed by horizon.
    #[serde(skip)]
    pub post: BTreeMap<usize, f64>,
    #[serde(skip)]
    pub pre: BTreeMap<&'static str, f64>,
}

fn stdev(xs: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let m = xs.iter().sum::<f64>() / xs.len() as f64;
    Some((xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64).sqrt())
}

pub fn window(def: &MarketDef, series: &Series, event: &Event) -> Option<MarketWindow> {
    let bars = &series.bars;
    let pre_idx = bars
        .iter()
        .rposition(|b| def.close_utc(b.date) < event.time_utc)?;
    if pre_idx < 252.min(bars.len() - 1) / 2 {
        return None; // not enough history for the pre-event state
    }
    let pre = bars[pre_idx].close;
    let m = def.measure;
    let d = m.decimals();
    let u = m.unit();
    let name = def.name;
    let mut facts = Vec::new();
    let mut pre_map = BTreeMap::new();

    let back = |k: usize| pre_idx.checked_sub(k).map(|i| bars[i].close);
    for (key, label, k) in [
        ("pre_ret5", "直前5営業日の変化", 5usize),
        ("pre_ret20", "直前20営業日の変化", 20),
    ] {
        if let Some(b) = back(k) {
            let v = m.change(b, pre);
            pre_map.insert(key, v);
            facts.push(Fact::signed(
                &format!("{}.{key}", def.id),
                &format!("{name} {label}"),
                v,
                d,
                u,
            ));
        }
    }
    // Daily variability over the 20 sessions before the event.
    if pre_idx >= 20 {
        let w = &bars[pre_idx - 20..=pre_idx];
        let diffs: Vec<f64> = w
            .windows(2)
            .map(|p| match m {
                Measure::Percent => (p[1].close / p[0].close).ln() * 100.0,
                _ => m.change(p[0].close, p[1].close),
            })
            .collect();
        if let Some(sd) = stdev(&diffs) {
            let (v, label, unit) = match m {
                Measure::Percent => (sd * 252f64.sqrt(), "直前20営業日の変動率（年率）", "%"),
                _ => (sd, "直前20営業日の日次変化の標準偏差", u),
            };
            pre_map.insert("pre_vol20", v);
            facts.push(Fact::num(
                &format!("{}.pre_vol20", def.id),
                &format!("{name} {label}"),
                v,
                d,
                unit,
            ));
        }
    }
    match m {
        Measure::Percent => {
            let lo = pre_idx.saturating_sub(251);
            let high = bars[lo..=pre_idx]
                .iter()
                .map(|b| b.close)
                .fold(f64::MIN, f64::max);
            let v = (pre / high - 1.0) * 100.0;
            pre_map.insert("pre_dd252", v);
            facts.push(Fact::signed(
                &format!("{}.pre_dd252", def.id),
                &format!("{name} 52週高値からの距離"),
                v,
                d,
                "%",
            ));
            if pre_idx >= 199 {
                let ma = bars[pre_idx - 199..=pre_idx]
                    .iter()
                    .map(|b| b.close)
                    .sum::<f64>()
                    / 200.0;
                let v = (pre / ma - 1.0) * 100.0;
                pre_map.insert("pre_ma200", v);
                facts.push(Fact::signed(
                    &format!("{}.pre_ma200", def.id),
                    &format!("{name} 200日移動平均との乖離"),
                    v,
                    d,
                    "%",
                ));
            }
        }
        Measure::Points => {
            pre_map.insert("pre_level", pre);
            facts.push(Fact::num(
                &format!("{}.pre_level", def.id),
                &format!("{name} 直前の水準"),
                pre,
                2,
                "",
            ));
        }
        Measure::BasisPoints => {
            pre_map.insert("pre_level", pre);
            facts.push(Fact::num(
                &format!("{}.pre_level", def.id),
                &format!("{name} 直前の水準"),
                pre,
                2,
                "%",
            ));
        }
    }

    let mut post = BTreeMap::new();
    for &h in HORIZONS {
        if let Some(b) = bars.get(pre_idx + h) {
            let v = m.change(pre, b.close);
            post.insert(h, v);
            facts.push(Fact::signed(
                &format!("{}.r{h}", def.id),
                &format!("{name} 発表後{h}本目の引けまでの変化"),
                v,
                d,
                u,
            ));
        }
    }
    let lo = pre_idx.saturating_sub(PRE_SESSIONS as usize);
    let hi = (pre_idx + POST_SESSIONS as usize).min(bars.len() - 1);
    let path = (lo..=hi)
        .map(|i| (i as i64 - pre_idx as i64, m.change(pre, bars[i].close)))
        .collect();
    Some(MarketWindow {
        market: def.id.into(),
        facts,
        path,
        post,
        pre: pre_map,
    })
}

/// How far the event has matured: `r5` once every wiki market has 5 post sessions, `r20` at 20.
pub fn stage(windows: &[MarketWindow]) -> Option<&'static str> {
    let wiki: Vec<&MarketWindow> = windows
        .iter()
        .filter(|w| crate::config::market(&w.market).is_some_and(|m| m.wiki))
        .collect();
    if wiki.is_empty() {
        return None;
    }
    if wiki.iter().all(|w| w.post.contains_key(&20)) {
        Some("r20")
    } else if wiki.iter().all(|w| w.post.contains_key(&5)) {
        Some("r5")
    } else {
        None
    }
}

// ── Aggregates by event type and sub-group ──────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub n: usize,
    pub mean: f64,
    pub median: f64,
    pub p25: f64,
    pub p75: f64,
    pub up_share: f64,
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    let pos = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

pub fn stats(xs: &[f64]) -> Option<Stats> {
    if xs.is_empty() {
        return None;
    }
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    Some(Stats {
        n: s.len(),
        mean: s.iter().sum::<f64>() / s.len() as f64,
        median: quantile(&s, 0.5),
        p25: quantile(&s, 0.25),
        p75: quantile(&s, 0.75),
        up_share: s.iter().filter(|x| **x > 0.0).count() as f64 / s.len() as f64 * 100.0,
    })
}

/// Sub-groups used to split each event type (the first is always "all").
pub fn groups(event: &Event, windows: &[MarketWindow]) -> Vec<(String, String)> {
    let mut g = vec![("all".to_string(), "全体".to_string())];
    let fact = |k: &str| event.facts.iter().find(|f| f.key == k);
    match event.kind.as_str() {
        "fomc" => {
            if let Some(f) = fact("decision") {
                let id = match f.text.as_str() {
                    "利上げ" => "hike",
                    "利下げ" => "cut",
                    _ => "hold",
                };
                g.push((format!("decision_{id}"), format!("{}のとき", f.text)));
            }
        }
        "cpi" => {
            if let (Some(a), Some(b)) = (
                fact("cpi_yoy").and_then(|f| f.value),
                fact("cpi_yoy_prev").and_then(|f| f.value),
            ) {
                let (id, label) = if (a - b).abs() < 0.05 {
                    ("cpi_flat", "前年比が前月と同じ")
                } else if a > b {
                    ("cpi_up", "前年比が前月から上昇")
                } else {
                    ("cpi_down", "前年比が前月から低下")
                };
                g.push((id.into(), label.into()));
            }
        }
        "nfp" => {
            if let (Some(a), Some(b)) = (
                fact("unemployment").and_then(|f| f.value),
                fact("unemployment_prev").and_then(|f| f.value),
            ) {
                let (id, label) = if (a - b).abs() < 0.05 {
                    ("ur_flat", "失業率が前月と同じ")
                } else if a > b {
                    ("ur_up", "失業率が前月から上昇")
                } else {
                    ("ur_down", "失業率が前月から低下")
                };
                g.push((id.into(), label.into()));
            }
        }
        _ => {}
    }
    if let Some(v) = windows
        .iter()
        .find(|w| w.market == "vix")
        .and_then(|w| w.pre.get("pre_level"))
    {
        let (id, label) = if *v < 15.0 {
            ("vix_low", "直前の VIX が 15 未満")
        } else if *v <= 25.0 {
            ("vix_mid", "直前の VIX が 15〜25")
        } else {
            ("vix_high", "直前の VIX が 25 超")
        };
        g.push((id.into(), label.into()));
    }
    g
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupAgg {
    pub id: String,
    pub label: String,
    pub n_events: usize,
    /// market → horizon → stats
    pub stats: BTreeMap<String, BTreeMap<usize, Stats>>,
    /// market → [(offset, mean, p25, p75)]
    pub path: BTreeMap<String, Vec<(i64, f64, f64, f64)>>,
}

pub fn aggregate(members: &[(&Event, &Vec<MarketWindow>)], id: &str, label: &str) -> GroupAgg {
    let mut stats_map = BTreeMap::new();
    let mut path_map = BTreeMap::new();
    for def in MARKETS {
        let ws: Vec<&MarketWindow> = members
            .iter()
            .filter_map(|(_, ws)| ws.iter().find(|w| w.market == def.id))
            .collect();
        let mut by_h = BTreeMap::new();
        for &h in HORIZONS {
            let xs: Vec<f64> = ws.iter().filter_map(|w| w.post.get(&h).copied()).collect();
            if let Some(s) = stats(&xs) {
                by_h.insert(h, s);
            }
        }
        if !by_h.is_empty() {
            stats_map.insert(def.id.to_string(), by_h);
        }
        let mut path = Vec::new();
        for k in -PRE_SESSIONS..=POST_SESSIONS {
            let xs: Vec<f64> = ws
                .iter()
                .filter_map(|w| w.path.iter().find(|(o, _)| *o == k).map(|(_, v)| *v))
                .collect();
            if let Some(s) = stats(&xs) {
                if s.n >= 3 || members.len() < 3 {
                    path.push((k, s.mean, s.p25, s.p75));
                }
            }
        }
        if !path.is_empty() {
            path_map.insert(def.id.to_string(), path);
        }
    }
    GroupAgg {
        id: id.into(),
        label: label.into(),
        n_events: members.len(),
        stats: stats_map,
        path: path_map,
    }
}

/// Facts for an aggregate (what the wiki may cite on event-type and market pages).
pub fn aggregate_facts(kind: &str, g: &GroupAgg) -> Vec<Fact> {
    let mut out = Vec::new();
    for (mid, by_h) in &g.stats {
        let Some(def) = crate::config::market(mid) else {
            continue;
        };
        let (d, u) = (def.measure.decimals(), def.measure.unit());
        for (h, s) in by_h {
            let base = format!("{kind}.{}.{mid}.r{h}", g.id);
            let what = format!(
                "{}（{}）{} 発表後{h}本目の引けまで",
                def.name,
                g.label,
                crate::config::event_type(kind)
                    .map(|t| t.name)
                    .unwrap_or(kind)
            );
            out.push(Fact::signed(
                &format!("{base}.mean"),
                &format!("{what} 平均"),
                s.mean,
                d,
                u,
            ));
            out.push(Fact::signed(
                &format!("{base}.median"),
                &format!("{what} 中央値"),
                s.median,
                d,
                u,
            ));
            out.push(Fact {
                key: format!("{base}.up_share"),
                label: format!("{what} 上昇した割合"),
                text: fmt_signed(s.up_share, 0, "%", false),
                value: Some(s.up_share),
            });
            out.push(Fact {
                key: format!("{base}.n"),
                label: format!("{what} 件数"),
                text: format!("{}件", s.n),
                value: Some(s.n as f64),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bar, Event, Series};
    use chrono::{NaiveDate, TimeZone, Utc};

    fn series(n: usize) -> Series {
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let bars = (0..n)
            .map(|i| Bar {
                date: start + chrono::Duration::days(i as i64),
                close: 100.0 + i as f64,
            })
            .collect();
        Series {
            id: "spx".into(),
            symbol: "^GSPC".into(),
            bars,
        }
    }

    #[test]
    fn pre_bar_is_last_close_before_event() {
        let s = series(400);
        let def = crate::config::market("spx").unwrap();
        // 2025-01-31 08:30 New York = 13:30 UTC, before that day's 16:00 close.
        let t = Utc.with_ymd_and_hms(2025, 1, 31, 13, 30, 0).unwrap();
        let ev = Event {
            id: "cpi-x".into(),
            kind: "cpi".into(),
            title: String::new(),
            date: t.date_naive(),
            time_utc: t,
            time_approx: false,
            facts: vec![],
            sources: vec![],
        };
        let w = window(def, &s, &ev).unwrap();
        let pre_idx = s
            .bars
            .iter()
            .position(|b| b.date == NaiveDate::from_ymd_opt(2025, 1, 30).unwrap())
            .unwrap();
        let expect = (s.bars[pre_idx + 1].close / s.bars[pre_idx].close - 1.0) * 100.0;
        assert!((w.post[&1] - expect).abs() < 1e-9);
        assert_eq!(w.path.iter().find(|(k, _)| *k == 0).unwrap().1, 0.0);
    }

    #[test]
    fn quantiles() {
        let s = stats(&[1.0, 2.0, 3.0, 4.0]).unwrap();
        assert_eq!((s.median, s.p25, s.up_share), (2.5, 1.75, 100.0));
    }
}
