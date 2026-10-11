//! `pup collect`: fetch prices and event calendars into `data/` (raw inputs, not committed).
//! A failing source keeps the previous run's output so one outage does not drop history.

use crate::config::MARKETS;
use crate::model::{Event, Series};
use crate::{http, sources};
use anyhow::Result;
use chrono::NaiveDate;
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
    // Release dates from FRED when a key is set, otherwise from the archived BLS release files.
    let fred_key = std::env::var("FRED_API_KEY").ok().filter(|k| !k.is_empty());
    let dates = |fred_id: u32, bls_name: &str| -> Result<Vec<NaiveDate>> {
        match &fred_key {
            Some(key) => sources::fred::release_dates(&client, key, fred_id, since),
            None => sources::wayback::bls_release_dates(&client, bls_name, since),
        }
    };
    match sources::bls::fetch(&client) {
        Ok(bls) => {
            for (kind, fred_id, bls_name) in [
                ("cpi", sources::fred::RELEASE_CPI, "cpi"),
                ("nfp", sources::fred::RELEASE_EMPLOYMENT, "empsit"),
                ("ppi", sources::fred::RELEASE_PPI, "ppi"),
                ("jolts", sources::fred::RELEASE_JOLTS, "jolts"),
            ] {
                keep(
                    kind,
                    dates(fred_id, bls_name).and_then(|d| sources::bls::events(kind, &d, &bls)),
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
    let mut events: Vec<Event> = previous
        .into_iter()
        .filter(|e| !by_kind.contains_key(e.kind.as_str()))
        .collect();
    events.extend(by_kind.into_values().flatten());
    events.retain(|e| e.date >= since && e.time_utc <= chrono::Utc::now());
    events.sort_by_key(|e| e.time_utc);
    events.dedup_by(|a, b| a.id == b.id);
    tracing::info!("events total: {}", events.len());
    write_json(&data.join("events.json"), &events)
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
