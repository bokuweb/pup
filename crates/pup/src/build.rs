//! `pup build`: turn raw inputs into the published facts (`public/data/`) that the SPA charts and
//! the wiki cite. Only derived values (changes, not price histories) are published.

use crate::analysis::{self, MarketWindow};
use crate::collect::{load_series, read_json, write_json};
use crate::config::{EVENT_TYPES, HORIZONS, MARKETS};
use crate::model::Event;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

pub fn run(data: &Path, out: &Path) -> Result<()> {
    let series = load_series(data);
    let events: Vec<Event> = read_json(&data.join("events.json"))
        .context("data/events.json (run `pup collect` first)")?;
    if out.exists() {
        std::fs::remove_dir_all(out)?;
    }
    let mut windows: Vec<(Event, Vec<MarketWindow>)> = Vec::new();
    for e in events {
        let ws: Vec<MarketWindow> = MARKETS
            .iter()
            .filter_map(|def| analysis::window(def, series.get(def.id)?, &e))
            .collect();
        if !ws.is_empty() {
            windows.push((e, ws));
        }
    }

    let mut list = Vec::new();
    for (e, ws) in &windows {
        let stage = analysis::stage(ws);
        let groups = analysis::groups(e, ws);
        write_json(
            &out.join("events").join(format!("{}.json", e.id)),
            &json!({"event": e, "stage": stage, "groups": groups, "markets": ws}),
        )?;
        let r1: BTreeMap<&str, f64> = ws
            .iter()
            .filter_map(|w| Some((w.market.as_str(), *w.post.get(&1)?)))
            .collect();
        list.push(json!({"id": e.id, "type": e.kind, "title": e.title, "date": e.date, "time_utc": e.time_utc, "stage": stage, "facts": e.facts, "r1": r1}));
    }
    list.reverse(); // newest first
    write_json(&out.join("events.json"), &list)?;

    for t in EVENT_TYPES {
        let members: Vec<(&Event, &Vec<MarketWindow>)> = windows
            .iter()
            .filter(|(e, _)| e.kind == t.id)
            .map(|(e, w)| (e, w))
            .collect();
        let mut group_ids: Vec<(String, String)> = Vec::new();
        for (e, ws) in &members {
            for g in analysis::groups(e, ws) {
                if !group_ids.contains(&g) {
                    group_ids.push(g);
                }
            }
        }
        let mut aggs = Vec::new();
        let mut facts = Vec::new();
        for (gid, label) in &group_ids {
            let ms: Vec<(&Event, &Vec<MarketWindow>)> = members
                .iter()
                .filter(|(e, ws)| analysis::groups(e, ws).iter().any(|(g, _)| g == gid))
                .copied()
                .collect();
            let a = analysis::aggregate(&ms, gid, label);
            facts.extend(analysis::aggregate_facts(t.id, &a));
            aggs.push(a);
        }
        write_json(
            &out.join("types").join(format!("{}.json", t.id)),
            &json!({"type": t, "n_events": members.len(), "groups": aggs, "facts": facts}),
        )?;
    }

    for def in MARKETS {
        let reactions: Vec<_> = windows
            .iter()
            .rev()
            .filter_map(|(e, ws)| {
                let w = ws.iter().find(|w| w.market == def.id)?;
                Some(json!({"event": e.id, "type": e.kind, "title": e.title, "date": e.date, "post": w.post, "pre": w.pre}))
            })
            .collect();
        write_json(
            &out.join("markets").join(format!("{}.json", def.id)),
            &json!({"market": def, "reactions": reactions}),
        )?;
    }

    write_json(
        &out.join("meta.json"),
        &json!({"generated_at": chrono::Utc::now(), "markets": MARKETS, "event_types": EVENT_TYPES, "horizons": HORIZONS, "n_events": windows.len()}),
    )?;
    tracing::info!("build: {} events → {}", windows.len(), out.display());
    Ok(())
}
