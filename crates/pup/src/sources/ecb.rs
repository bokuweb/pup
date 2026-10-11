//! ECB Governing Council monetary policy decisions: dates from the yearly press listings, rate
//! levels from the ECB Data Portal (key rates by effective date, which is the Wednesday after the
//! decision). The decision is read from the deposit facility rate, the ECB's policy rate since 2019.

use crate::http;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone};
use regex::Regex;
use std::collections::BTreeMap;

const BASE: &str = "https://www.ecb.europa.eu";
const FIRST_YEAR: i32 = 2016;
const RATES: &str = "https://data-api.ecb.europa.eu/service/data/FM/B.U2.EUR.4F.KR.DFR+MRR_FR.LEV?format=csvdata&detail=dataonly&startPeriod=2014-01-01";

/// One listing entry; every part stays inside its own element so an entry never borrows the
/// link of the next one.
fn listing_re() -> Regex {
    Regex::new(
        r#"(?i)<dt isoDate="(\d{4}-\d{2}-\d{2})">\s*<div class="date">[^<]*</div>\s*</dt>\s*<dd>\s*<div class="title">\s*<a href="(/press/pr/date/[^"]+)"[^>]*>\s*Monetary policy decisions\s*<"#,
    )
    .unwrap()
}

/// Decision dates and press-release URLs. The listings also hold accounts and other releases, so
/// entries are kept only when titled "Monetary policy decisions" (this drops the PEPP release of
/// 18 March 2020, which did not change rates).
pub fn decision_dates(client: &reqwest::blocking::Client) -> Result<BTreeMap<NaiveDate, String>> {
    let re = listing_re();
    let mut out = BTreeMap::new();
    for y in FIRST_YEAR..=chrono::Utc::now().year() {
        let url = format!("{BASE}/press/govcdec/mopo/{y}/html/index_include.en.html");
        match http::get_text(client, &url) {
            Ok(html) => {
                for c in re.captures_iter(&html) {
                    if let Ok(d) = NaiveDate::parse_from_str(&c[1], "%Y-%m-%d") {
                        out.entry(d).or_insert_with(|| format!("{BASE}{}", &c[2]));
                    }
                }
            }
            Err(e) => tracing::warn!("ecb {url}: {e:#}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Ok(out)
}

/// `series → [(effective date, level)]` from the Data Portal CSV.
pub fn parse_rates(csv: &str) -> BTreeMap<String, Vec<(NaiveDate, f64)>> {
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let col = |name: &str| header.iter().position(|h| *h == name);
    let (Some(k), Some(t), Some(v)) = (col("KEY"), col("TIME_PERIOD"), col("OBS_VALUE")) else {
        return BTreeMap::new();
    };
    let mut out: BTreeMap<String, Vec<(NaiveDate, f64)>> = BTreeMap::new();
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        let (Some(key), Some(date), Some(val)) = (f.get(k), f.get(t), f.get(v)) else {
            continue;
        };
        let series = key.split('.').nth(6).unwrap_or(key).to_string();
        if let (Ok(d), Ok(x)) = (NaiveDate::parse_from_str(date, "%Y-%m-%d"), val.parse()) {
            out.entry(series).or_default().push((d, x));
        }
    }
    for xs in out.values_mut() {
        xs.sort_by_key(|(d, _)| *d);
    }
    out
}

/// Level in effect on `date`.
fn level_on(xs: &[(NaiveDate, f64)], date: NaiveDate) -> Option<f64> {
    xs.iter().rev().find(|(d, _)| *d <= date).map(|(_, v)| *v)
}

/// Before and after a decision: the level on the decision day and the one a week later, when
/// any change has taken effect (the next meeting is always at least three weeks away).
pub fn before_after(xs: &[(NaiveDate, f64)], date: NaiveDate) -> Option<(f64, f64)> {
    Some((level_on(xs, date)?, level_on(xs, date + Duration::days(7))?))
}

/// 13:45 CET until June 2022, 14:15 from July 2022 (press conference moved to 14:45).
pub fn release_time(date: NaiveDate) -> NaiveTime {
    if date < NaiveDate::from_ymd_opt(2022, 7, 1).unwrap() {
        NaiveTime::from_hms_opt(13, 45, 0).unwrap()
    } else {
        NaiveTime::from_hms_opt(14, 15, 0).unwrap()
    }
}

pub fn collect(client: &reqwest::blocking::Client) -> Result<Vec<Event>> {
    let dates = decision_dates(client)?;
    let rates = parse_rates(&http::get_text(client, RATES)?);
    let dfr = rates
        .get("DFR")
        .context("ECB deposit facility rate missing")?;
    let mro = rates.get("MRR_FR");
    let frankfurt: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let mut events = Vec::new();
    for (date, url) in dates {
        if date > chrono::Utc::now().date_naive() {
            continue;
        }
        let Some((before, after)) = before_after(dfr, date) else {
            tracing::warn!("ecb {date}: no deposit rate");
            continue;
        };
        let change_bp = ((after - before) * 100.0).round();
        let decision = if change_bp > 0.0 {
            "利上げ"
        } else if change_bp < 0.0 {
            "利下げ"
        } else {
            "据え置き"
        };
        let mut facts = vec![
            Fact::text("decision", "決定", decision),
            Fact::text(
                "deposit_rate",
                "預金ファシリティ金利",
                format!("{after:.2}%"),
            ),
            Fact::signed("change_bp", "変更幅", change_bp, 0, "bp"),
            Fact::text("prev_rate", "変更前", format!("{before:.2}%")),
        ];
        if let Some((_, m)) = mro.and_then(|m| before_after(m, date)) {
            facts.push(Fact::text(
                "mro_rate",
                "主要リファイナンス金利",
                format!("{m:.2}%"),
            ));
        }
        let time_utc = frankfurt
            .from_local_datetime(&date.and_time(release_time(date)))
            .earliest()
            .context("release time")?
            .with_timezone(&chrono::Utc);
        events.push(Event {
            id: format!("ecb-{date}"),
            kind: "ecb".into(),
            title: format!("ECB 理事会 {}（{decision}）", date.format("%Y年%-m月%-d日")),
            date,
            time_utc,
            time_approx: false,
            facts,
            sources: vec![
                SourceRef {
                    label: "ECB press release".into(),
                    url,
                },
                SourceRef {
                    label: "ECB Data Portal (key interest rates)".into(),
                    url: "https://data.ecb.europa.eu/main-figures/ecb-interest-rates-and-exchange-rates/key-ecb-interest-rates".into(),
                },
            ],
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn joins_decisions_with_effective_dates() {
        let csv = "KEY,FREQ,REF_AREA,CURRENCY,PROVIDER_FM,INSTRUMENT_FM,PROVIDER_FM_ID,DATA_TYPE_FM,TIME_PERIOD,OBS_VALUE\n\
FM.B.U2.EUR.4F.KR.DFR.LEV,B,U2,EUR,4F,KR,DFR,LEV,2024-06-12,3.75\n\
FM.B.U2.EUR.4F.KR.DFR.LEV,B,U2,EUR,4F,KR,DFR,LEV,2024-09-18,3.5\n\
FM.B.U2.EUR.4F.KR.MRR_FR.LEV,B,U2,EUR,4F,KR,MRR_FR,LEV,2024-09-18,3.65\n";
        let r = parse_rates(csv);
        let dfr = &r["DFR"];
        // Decided on Thursday 12 September, effective Wednesday 18 September.
        assert_eq!(before_after(dfr, d("2024-09-12")), Some((3.75, 3.5)));
        assert_eq!(before_after(dfr, d("2024-07-18")), Some((3.75, 3.75)));
        assert_eq!(r["MRR_FR"], vec![(d("2024-09-18"), 3.65)]);
    }

    #[test]
    fn listing_keeps_only_decisions() {
        let html = r#"<dt isoDate="2024-12-12"><div class="date">12 December 2024</div></dt><dd><div class="title"><a href="/press/pr/date/2024/html/ecb.mp241212~2acab6e51e.en.html">Monetary policy decisions</a></div></dd>
<dt isoDate="2025-01-16"><div class="date">16 January 2025</div></dt><dd><div class="title"><a href="/press/accounts/2025/html/ecb.mg250116.en.html">Account of the monetary policy meeting</a></div></dd>"#;
        let got: Vec<_> = listing_re()
            .captures_iter(html)
            .map(|c| c[1].to_string())
            .collect();
        assert_eq!(got, vec!["2024-12-12"]);
    }

    #[test]
    fn release_time_changed_in_2022() {
        assert_eq!(
            release_time(d("2022-06-09")),
            NaiveTime::from_hms_opt(13, 45, 0).unwrap()
        );
        assert_eq!(
            release_time(d("2022-07-21")),
            NaiveTime::from_hms_opt(14, 15, 0).unwrap()
        );
    }
}
