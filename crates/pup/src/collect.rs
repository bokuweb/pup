//! `pup collect`: fetch prices and event calendars into `data/` (raw inputs, not committed).
//! A failing source keeps the previous run's output, and events are merged by id, so one outage
//! does not drop history.

use crate::config::MARKETS;
use crate::model::{Event, Series};
use crate::{http, sources};
use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use std::collections::BTreeMap;
use std::path::Path;

pub fn run(data: &Path, since: NaiveDate) -> Result<()> {
    let client = http::client()?;
    std::fs::create_dir_all(data.join("prices"))?;
    // Enough history before `since` for the 200-day average and the 52-week high.
    let from = since - chrono::Duration::days(400);
    for def in MARKETS {
        match sources::yahoo::fetch(&client, def, from) {
            Ok(s) if !s.bars.is_empty() => {
                tracing::info!(
                    "prices {}: {} bars, last {}",
                    def.id,
                    s.bars.len(),
                    s.bars.last().unwrap().date
                );
                write_json(&data.join("prices").join(format!("{}.json", def.id)), &s)?;
            }
            Ok(_) => tracing::warn!("prices {}: empty, keeping previous", def.id),
            Err(e) => tracing::warn!("prices {}: {e:#}, keeping previous", def.id),
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }

    let previous: Vec<Event> = read_json(&data.join("events.json")).unwrap_or_default();
    let mut by_kind: BTreeMap<&str, Vec<Event>> = BTreeMap::new();
    let mut keep = |kind: &'static str, r: Result<Vec<Event>>| match r {
        Ok(v) if !v.is_empty() => {
            tracing::info!("events {kind}: {}", v.len());
            by_kind.insert(kind, v);
        }
        Ok(_) => tracing::warn!("events {kind}: none found"),
        Err(e) => tracing::warn!("events {kind}: {e:#}, keeping previous"),
    };
    keep(
        "fomc",
        sources::fomc::collect(&client, &data.join("cache/fomc")),
    );
    keep("boj", sources::boj::collect(&client));
    keep("ecb", sources::ecb::collect(&client));
    keep(
        "boe",
        sources::boe::collect(&client, &data.join("cache/boe")),
    );
    keep(
        "jpcpi",
        sources::jpcpi::releases(&client, &data.join("cache/jpcpi"))
            .and_then(|r| sources::jpcpi::events(&r)),
    );
    keep(
        "tankan",
        sources::tankan::surveys(&client, &data.join("cache/tankan"))
            .and_then(|s| sources::tankan::events(&s)),
    );
    // Release dates from FRED when a key is set, otherwise from the archived BLS release files plus,
    // for the latest releases the Archive has not crawled yet, the BLS monthly calendars (an
    // event only appears once BLS data for its reference month exist).
    let fred_key = std::env::var("FRED_API_KEY").ok().filter(|k| !k.is_empty());
    let today = chrono::Utc::now().date_naive();
    let schedule = if fred_key.is_none() {
        let prev = today.with_day(1).unwrap() - chrono::Duration::days(1);
        sources::wayback::bls_schedule(
            &client,
            &[(prev.year(), prev.month()), (today.year(), today.month())],
        )
    } else {
        Vec::new()
    };
    let dates = |fred_id: u32, kind: &str, bls_name: &str, title: &str| -> Result<Vec<NaiveDate>> {
        match &fred_key {
            Some(key) => sources::fred::release_dates(&client, key, fred_id, since),
            None => {
                let mut d = sources::wayback::bls_release_dates(&client, bls_name, since)?;
                let covered: std::collections::BTreeSet<(i32, u32)> = d
                    .iter()
                    .filter_map(|x| sources::bls::reference_month(kind, *x).ok())
                    .collect();
                for s in &schedule {
                    if s.title == title && s.date <= today && !covered.contains(&s.reference) {
                        d.push(s.date);
                    }
                }
                d.sort();
                d.dedup();
                Ok(d)
            }
        }
    };
    match sources::bls::fetch(&client) {
        Ok(bls) => {
            for (kind, fred_id, bls_name, title) in [
                (
                    "cpi",
                    sources::fred::RELEASE_CPI,
                    "cpi",
                    "Consumer Price Index",
                ),
                (
                    "nfp",
                    sources::fred::RELEASE_EMPLOYMENT,
                    "empsit",
                    "Employment Situation",
                ),
                (
                    "ppi",
                    sources::fred::RELEASE_PPI,
                    "ppi",
                    "Producer Price Index",
                ),
                (
                    "jolts",
                    sources::fred::RELEASE_JOLTS,
                    "jolts",
                    "Job Openings and Labor Turnover Survey",
                ),
            ] {
                keep(
                    kind,
                    dates(fred_id, kind, bls_name, title)
                        .and_then(|d| sources::bls::events(kind, &d, &bls)),
                );
                std::thread::sleep(std::time::Duration::from_millis(1000));
            }
        }
        Err(e) => {
            for kind in ["cpi", "nfp", "ppi", "jolts"] {
                keep(kind, Err(anyhow::anyhow!("BLS: {e:#}")));
            }
        }
    }
    keep(
        "retail",
        sources::census::releases(&client, &data.join("cache/retail"), since)
            .and_then(|r| sources::census::events(&r)),
    );
    keep(
        "gdp",
        sources::bea::gdp_events(&client, &data.join("cache/bea"), since),
    );
    keep(
        "pce",
        sources::bea::pce_events(&client, &data.join("cache/bea"), since),
    );
    // New events replace previous ones with the same id; previous events a run did not return are
    // kept, so a page that failed to load this time does not drop history.
    let fresh: std::collections::BTreeSet<String> =
        by_kind.values().flatten().map(|e| e.id.clone()).collect();
    let mut events: Vec<Event> = previous
        .into_iter()
        .filter(|e| !fresh.contains(&e.id))
        .collect();
    events.extend(by_kind.into_values().flatten());
    events.retain(|e| e.date >= since && e.time_utc <= chrono::Utc::now());
    events.sort_by_key(|e| e.time_utc);
    events.dedup_by(|a, b| a.id == b.id);
    tracing::info!("events total: {}", events.len());
    report_stale(&events);
    write_json(&data.join("events.json"), &events)
}

/// Monthly releases whose latest event is older than this many days are flagged: a source can
/// keep "succeeding" with old data (a lagging archive, a changed page), and the run would still
/// look green.
const STALE_DAYS: i64 = 45;
const MONTHLY: &[&str] = &["cpi", "nfp", "ppi", "jolts", "retail", "pce", "jpcpi"];

fn report_stale(events: &[Event]) {
    let today = chrono::Utc::now().date_naive();
    for kind in MONTHLY {
        let last = events
            .iter()
            .filter(|e| e.kind == *kind)
            .map(|e| e.date)
            .max();
        let stale = last.is_none_or(|d| (today - d).num_days() > STALE_DAYS);
        if stale {
            let msg = format!(
                "{kind}: latest release {} is more than {STALE_DAYS} days old",
                last.map_or("none".to_string(), |d| d.to_string())
            );
            tracing::warn!("{msg}");
            if std::env::var("GITHUB_ACTIONS").is_ok() {
                println!("::warning title=stale events::{msg}");
            }
        }
    }
}

pub fn write_json<T: serde::Serialize>(path: &Path, v: &T) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, serde_json::to_vec(v)?)?;
    Ok(())
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn load_series(data: &Path) -> BTreeMap<String, Series> {
    MARKETS
        .iter()
        .filter_map(|m| {
            read_json::<Series>(&data.join("prices").join(format!("{}.json", m.id)))
                .map(|s| (m.id.to_string(), s))
        })
        .collect()
}
