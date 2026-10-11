//! US CPI, Employment Situation, PPI and JOLTS figures from the BLS public API (v1, no key,
//! 10 years per call).
//! Values are the current published ones (later revisions included), not the first print.

use crate::config;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{Datelike, Months, NaiveDate, NaiveTime, TimeZone};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const CPI_ALL: &str = "CUUR0000SA0";
const CPI_CORE: &str = "CUUR0000SA0L1E";
/// Seasonally adjusted, for the month-on-month rates the release leads with.
const CPI_ALL_SA: &str = "CUSR0000SA0";
const CPI_CORE_SA: &str = "CUSR0000SA0L1E";
const PAYROLLS: &str = "CES0000000001";
const UNEMPLOYMENT: &str = "LNS14000000";
/// PPI final demand, seasonally adjusted (the headline m/m) and not adjusted (y/y).
const PPI_SA: &str = "WPSFD4";
const PPI_NSA: &str = "WPUFD4";
/// JOLTS job openings, total nonfarm, seasonally adjusted, thousands.
const OPENINGS: &str = "JTS000000000000000JOL";

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
    let body = json!({"seriesid": [CPI_ALL, CPI_CORE, CPI_ALL_SA, CPI_CORE_SA, PAYROLLS, UNEMPLOYMENT, PPI_SA, PPI_NSA, OPENINGS], "startyear": from.to_string(), "endyear": to.to_string()});
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

/// The month a release covers: the month before, except the releases delayed by the 2025 shutdown
/// (Employment Situation: September's on 20 November, October's and November's together on
/// 16 December; PPI: September's on 25 November, October's and November's on 14 January,
/// December's on 30 January; a combined release is labelled with its later month). JOLTS lags
/// five to seven weeks, so its month is the one 48 days before the release; this gives
/// consecutive months for every release since 2016 apart from the combined one of 9 December 2025.
pub fn reference_month(kind: &str, date: NaiveDate) -> Result<(i32, u32)> {
    match (kind, date.to_string().as_str()) {
        ("nfp", "2025-11-20") => Ok((2025, 9)),
        ("nfp", "2025-12-16") => Ok((2025, 11)),
        ("ppi", "2025-11-25") => Ok((2025, 9)),
        ("ppi", "2026-01-14") => Ok((2025, 11)),
        ("ppi", "2026-01-30") => Ok((2025, 12)),
        ("jolts", _) => Ok(ym(date - chrono::Duration::days(48))),
        _ => Ok(ym(date
            .checked_sub_months(Months::new(1))
            .context("month")?)),
    }
}

fn yoy(series: &BTreeMap<(i32, u32), f64>, m: (i32, u32)) -> Option<f64> {
    Some((series.get(&m)? / series.get(&(m.0 - 1, m.1))? - 1.0) * 100.0)
}

/// Month-on-month change rounded to the published decimal.
fn mom(series: &BTreeMap<(i32, u32), f64>, m: (i32, u32)) -> Option<f64> {
    let v = (series.get(&m)? / series.get(&prev_month(m))? - 1.0) * 100.0;
    Some((v * 10.0).round() / 10.0)
}

fn prev_month(m: (i32, u32)) -> (i32, u32) {
    if m.1 == 1 {
        (m.0 - 1, 12)
    } else {
        (m.0, m.1 - 1)
    }
}

/// 08:30 New York, except JOLTS at 10:00.
fn release_time(kind: &str, date: NaiveDate) -> Result<chrono::DateTime<chrono::Utc>> {
    let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
    let t = if kind == "jolts" {
        NaiveTime::from_hms_opt(10, 0, 0).unwrap()
    } else {
        NaiveTime::from_hms_opt(8, 30, 0).unwrap()
    };
    Ok(ny
        .from_local_datetime(&date.and_time(t))
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
                if let Some(m) = mom(get(CPI_ALL_SA), refm) {
                    f.push(Fact::signed(
                        "cpi_mom",
                        "CPI 前月比（季節調整済み）",
                        m,
                        1,
                        "%",
                    ));
                }
                if let Some(m) = mom(get(CPI_CORE_SA), refm) {
                    f.push(Fact::signed(
                        "core_mom",
                        "コア CPI 前月比（季節調整済み）",
                        m,
                        1,
                        "%",
                    ));
                }
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
            "ppi" => {
                let (Some(m), Some(y)) = (mom(get(PPI_SA), refm), yoy(get(PPI_NSA), refm)) else {
                    continue;
                };
                let mut f = vec![
                    Fact::text("reference", "対象月", label.clone()),
                    Fact::signed("ppi_mom", "PPI 最終需要 前月比（季節調整済み）", m, 1, "%"),
                    Fact::num("ppi_yoy", "PPI 最終需要 前年比", y, 1, "%"),
                ];
                if let Some(pm) = mom(get(PPI_SA), prev_month(refm)) {
                    f.push(Fact::signed(
                        "ppi_mom_prev",
                        "PPI 前月比（前月）",
                        pm,
                        1,
                        "%",
                    ));
                }
                f
            }
            "jolts" => {
                let o = get(OPENINGS);
                let Some(now) = o.get(&refm) else {
                    continue;
                };
                let mut f = vec![
                    Fact::text("reference", "対象月", label.clone()),
                    Fact::num("openings", "求人件数（季節調整済み）", *now, 0, "千件"),
                ];
                if let Some(before) = o.get(&prev_month(refm)) {
                    f.push(Fact::num(
                        "openings_prev",
                        "求人件数（前月）",
                        *before,
                        0,
                        "千件",
                    ));
                    f.push(Fact::signed(
                        "openings_change",
                        "求人件数の前月差",
                        now - before,
                        0,
                        "千件",
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
            time_utc: release_time(kind, date)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn reference_months() {
        assert_eq!(reference_month("cpi", d("2026-09-11")).unwrap(), (2026, 8));
        assert_eq!(reference_month("nfp", d("2025-12-16")).unwrap(), (2025, 11));
        assert_eq!(reference_month("ppi", d("2026-01-30")).unwrap(), (2025, 12));
        // JOLTS: early-month releases cover two months back, late-month ones the month before.
        assert_eq!(
            reference_month("jolts", d("2016-01-12")).unwrap(),
            (2015, 11)
        );
        assert_eq!(
            reference_month("jolts", d("2026-09-01")).unwrap(),
            (2026, 7)
        );
        assert_eq!(
            reference_month("jolts", d("2026-09-29")).unwrap(),
            (2026, 8)
        );
        assert_eq!(
            reference_month("jolts", d("2025-12-09")).unwrap(),
            (2025, 10)
        );
    }
}
