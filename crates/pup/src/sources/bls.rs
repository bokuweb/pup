//! US CPI and Employment Situation figures from the BLS public API (v1, no key, 10 years per call).
//! Values are the current published ones (later revisions included), not the first print.

use crate::config;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{Datelike, Months, NaiveDate, NaiveTime, TimeZone};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const CPI_ALL: &str = "CUUR0000SA0";
const CPI_CORE: &str = "CUUR0000SA0L1E";
const PAYROLLS: &str = "CES0000000001";
const UNEMPLOYMENT: &str = "LNS14000000";

type Monthly = BTreeMap<String, BTreeMap<(i32, u32), f64>>;

/// First year fetched: the January 2016 release (December 2015) needs December 2014 for its y/y.
const FIRST_YEAR: i32 = 2014;

/// v1 serves at most ten years per call (and about 25 calls a day), so the span is split.
pub fn fetch(client: &reqwest::blocking::Client) -> Result<Monthly> {
    let end = chrono::Utc::now().year();
    let mut out = Monthly::new();
    let mut to = end;
    while to >= FIRST_YEAR {
        let from = (to - 9).max(FIRST_YEAR);
        fetch_span(client, from, to, &mut out)?;
        to = from - 1;
    }
    Ok(out)
}

fn fetch_span(
    client: &reqwest::blocking::Client,
    from: i32,
    to: i32,
    out: &mut Monthly,
) -> Result<()> {
    let body = json!({"seriesid": [CPI_ALL, CPI_CORE, PAYROLLS, UNEMPLOYMENT], "startyear": from.to_string(), "endyear": to.to_string()});
    let resp = client
        .post("https://api.bls.gov/publicAPI/v1/timeseries/data/")
        .json(&body)
        .send()?
        .error_for_status()?;
    let v: Value = resp.json()?;
    if v["status"] != "REQUEST_SUCCEEDED" {
        anyhow::bail!("BLS: {}", v["message"]);
    }
    for s in v["Results"]["series"].as_array().context("series")? {
        let id = s["seriesID"].as_str().unwrap_or_default().to_string();
        let m = out.entry(id).or_default();
        for d in s["data"].as_array().into_iter().flatten() {
            let (Some(y), Some(p), Some(val)) = (
                d["year"].as_str(),
                d["period"].as_str(),
                d["value"].as_str(),
            ) else {
                continue;
            };
            let (Ok(y), Some(Ok(mo))) =
                (y.parse::<i32>(), p.strip_prefix('M').map(str::parse::<u32>))
            else {
                continue;
            };
            if (1..=12).contains(&mo) {
                if let Ok(x) = val.parse::<f64>() {
                    m.insert((y, mo), x);
                }
            }
        }
    }
    Ok(())
}

fn ym(d: NaiveDate) -> (i32, u32) {
    (d.year(), d.month())
}

/// The month a release covers: the month before, except the Employment Situation releases delayed
/// by the 2025 shutdown (September's on 20 November; October's and November's together on
/// 16 December, whose headline is November).
pub fn reference_month(kind: &str, date: NaiveDate) -> Result<(i32, u32)> {
    match (kind, date.to_string().as_str()) {
        ("nfp", "2025-11-20") => Ok((2025, 9)),
        ("nfp", "2025-12-16") => Ok((2025, 11)),
        _ => Ok(ym(date
            .checked_sub_months(Months::new(1))
            .context("month")?)),
    }
}

fn yoy(series: &BTreeMap<(i32, u32), f64>, m: (i32, u32)) -> Option<f64> {
    Some((series.get(&m)? / series.get(&(m.0 - 1, m.1))? - 1.0) * 100.0)
}

fn prev_month(m: (i32, u32)) -> (i32, u32) {
    if m.1 == 1 {
        (m.0 - 1, 12)
    } else {
        (m.0, m.1 - 1)
    }
}

fn release_time(date: NaiveDate) -> Result<chrono::DateTime<chrono::Utc>> {
    let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
    Ok(ny
        .from_local_datetime(&date.and_time(NaiveTime::from_hms_opt(8, 30, 0).unwrap()))
        .earliest()
        .context("time")?
        .with_timezone(&chrono::Utc))
}

/// Builds events for release dates whose reference month (the month before release) has data.
pub fn events(kind: &str, dates: &[NaiveDate], data: &Monthly) -> Result<Vec<Event>> {
    let empty = BTreeMap::new();
    let get = |id: &str| data.get(id).unwrap_or(&empty);
    let mut out = Vec::new();
    for &date in dates {
        let refm = reference_month(kind, date)?;
        let label = format!("{}年{}月分", refm.0, refm.1);
        let facts = match kind {
            "cpi" => {
                let (Some(h), Some(c)) = (yoy(get(CPI_ALL), refm), yoy(get(CPI_CORE), refm)) else {
                    continue;
                };
                let mut f = vec![
                    Fact::text("reference", "対象月", label.clone()),
                    Fact::num("cpi_yoy", "CPI 前年比", h, 1, "%"),
                    Fact::num("core_yoy", "コア CPI 前年比", c, 1, "%"),
                ];
                if let (Some(ph), Some(pc)) = (
                    yoy(get(CPI_ALL), prev_month(refm)),
                    yoy(get(CPI_CORE), prev_month(refm)),
                ) {
                    f.push(Fact::num("cpi_yoy_prev", "CPI 前年比（前月）", ph, 1, "%"));
                    f.push(Fact::num(
                        "core_yoy_prev",
                        "コア CPI 前年比（前月）",
                        pc,
                        1,
                        "%",
                    ));
                }
                f
            }
            "nfp" => {
                let p = get(PAYROLLS);
                let (Some(now), Some(before), Some(u)) = (
                    p.get(&refm),
                    p.get(&prev_month(refm)),
                    get(UNEMPLOYMENT).get(&refm),
                ) else {
                    continue;
                };
                let mut f = vec![
                    Fact::text("reference", "対象月", label.clone()),
                    Fact::signed(
                        "payrolls_change",
                        "非農業部門雇用者数（前月差、千人）",
                        now - before,
                        0,
                        "千人",
                    ),
                    Fact::num("unemployment", "失業率", *u, 1, "%"),
                ];
                if let Some(pu) = get(UNEMPLOYMENT).get(&prev_month(refm)) {
                    f.push(Fact::num(
                        "unemployment_prev",
                        "失業率（前月）",
                        *pu,
                        1,
                        "%",
                    ));
                }
                f
            }
            _ => unreachable!(),
        };
        let name = config::event_type(kind).map(|t| t.name).unwrap_or(kind);
        out.push(Event {
            id: format!("{kind}-{date}"),
            kind: kind.into(),
            title: format!("{name} {}（{label}）", date.format("%Y年%-m月%-d日")),
            date,
            time_utc: release_time(date)?,
            time_approx: false,
            facts,
            sources: vec![SourceRef {
                label: "BLS public data API（現在の公表値。速報値ではない）".into(),
                url: "https://www.bls.gov/developers/".into(),
            }],
        });
    }
    Ok(out)
}
