//! US advance retail sales (Census Bureau, Advance Monthly Retail Trade): one PDF per reference
//! month (`advYYMM.pdf`) listed on the historic releases page. Page 1 gives the release date and
//! the headline month-on-month change as first published.

use crate::http;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveTime, TimeZone};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const LIST: &str = "https://www.census.gov/retail/marts/historic_releases.html";
/// The latest report, listed on the historic page only after the next one is out.
const CURRENT: &str = "https://www.census.gov/retail/marts/www/marts_current.pdf";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// `YYYY-MM` of the reference month.
    pub month: String,
    pub released: NaiveDate,
    pub mom: f64,
    pub url: String,
}

/// `(YYMM, url)` of every advance report linked from the list.
pub fn report_links(html: &str) -> BTreeMap<String, String> {
    let re = Regex::new(
        r#"href="(https://www2\.census\.gov/retail/releases/historical/marts/adv(\d{4})\.pdf)""#,
    )
    .unwrap();
    re.captures_iter(html)
        .map(|c| (c[2].to_string(), c[1].to_string()))
        .collect()
}

/// Release date (`FOR RELEASE AT 8:30 AM EDT, MONDAY, JULY 16, 2018`, or
/// `FRIDAY, FEBRUARY 12, 2016, AT 8:30 A.M. EST`) and the m/m change
/// (`were $506.8 billion, an increase of 0.5 percent` / `up` / `down` / `virtually unchanged`; the
/// table layout of some reports: `$773.9 billion $764.5 billion 1.2% -0.5%`).
pub fn parse_report(text: &str) -> Option<(NaiveDate, f64)> {
    let flat = Regex::new(r"\s+")
        .unwrap()
        .replace_all(text, " ")
        .to_string();
    let d = Regex::new(
        r"(?i)(?:MONDAY|TUESDAY|WEDNESDAY|THURSDAY|FRIDAY),? ([A-Z]+) (\d{1,2}), (\d{4})",
    )
    .unwrap();
    let c = d.captures(&flat)?;
    let released =
        NaiveDate::parse_from_str(&format!("{} {} {}", &c[1], &c[2], &c[3]), "%B %d %Y").ok()?;
    let prose = Regex::new(
        r"were \$ ?[\d.,]+ billion,? (?:(an increase of|a decrease of|up|down) ([\d.]+) ?(?:percent|%)|(virtually unchanged))",
    )
    .unwrap();
    let mom = if let Some(m) = prose.captures(&flat) {
        match (m.get(1), m.get(2)) {
            (Some(dir), Some(v)) => {
                let v: f64 = v.as_str().parse().ok()?;
                if matches!(dir.as_str(), "a decrease of" | "down") {
                    -v
                } else {
                    v
                }
            }
            _ => 0.0,
        }
    } else {
        let table = Regex::new(r"\$[\d.,]+ billion \$[\d.,]+ billion (-?[\d.]+)%").unwrap();
        table.captures(&flat)?[1].parse().ok()?
    };
    Some((released, mom))
}

pub fn releases(
    client: &reqwest::blocking::Client,
    cache: &Path,
    since: NaiveDate,
) -> Result<Vec<Release>> {
    std::fs::create_dir_all(cache)?;
    let links = report_links(&http::get_text(client, LIST)?);
    anyhow::ensure!(!links.is_empty(), "no advance reports listed");
    // A release covers the month before it, so start one month before `since`.
    let first = (since - chrono::Duration::days(40))
        .format("%Y-%m")
        .to_string();
    let this_month = chrono::Utc::now().format("%Y-%m").to_string();
    let mut out = Vec::new();
    for (yymm, url) in links {
        // The list goes back to 1992 (`adv9201`).
        let century = if yymm[..2] >= *"90" { "19" } else { "20" };
        let month = format!("{century}{}-{}", &yymm[..2], &yymm[2..]);
        // The list also links reports whose names do not follow `advYYMM` for a past month.
        if month < first || month > this_month {
            continue;
        }
        let cached = cache.join(format!("{month}.json"));
        if let Some(r) = std::fs::read(&cached)
            .ok()
            .and_then(|b| serde_json::from_slice::<Release>(&b).ok())
        {
            out.push(r);
            continue;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        let r = (|| -> Result<Release> {
            let pdf = http::get_bytes(client, &url)?;
            // The site sometimes answers with an HTML "Request Rejected" page; retry on the next run.
            anyhow::ensure!(pdf.starts_with(b"%PDF"), "not a PDF (rejected?)");
            let text = http::pdf_first_page(&pdf, false)?;
            let (released, mom) = parse_report(&text).context("no date or headline")?;
            Ok(Release {
                month: month.clone(),
                released,
                mom,
                url: url.clone(),
            })
        })();
        match r {
            Ok(r) => {
                std::fs::write(&cached, serde_json::to_vec(&r)?)?;
                out.push(r);
            }
            Err(e) => tracing::warn!("retail {month}: {e:#}"),
        }
    }
    match current(client) {
        Ok(r) if !out.iter().any(|x| x.month == r.month) => out.push(r),
        Ok(_) => {}
        Err(e) => tracing::warn!("retail current report: {e:#}"),
    }
    out.retain(|r| r.released >= since);
    out.sort_by_key(|r| r.released);
    Ok(out)
}

/// The latest report (not cached: the same URL serves next month's report later).
fn current(client: &reqwest::blocking::Client) -> Result<Release> {
    let pdf = http::get_bytes(client, CURRENT)?;
    anyhow::ensure!(pdf.starts_with(b"%PDF"), "not a PDF (rejected?)");
    let text = http::pdf_first_page(&pdf, false)?;
    let (released, mom) = parse_report(&text).context("no date or headline")?;
    let month = reference_month(&text).context("no reference month")?;
    Ok(Release {
        month,
        released,
        mom,
        url: CURRENT.into(),
    })
}

/// `ADVANCE MONTHLY SALES FOR RETAIL AND FOOD SERVICES, SEPTEMBER 2026` → `2026-09`.
pub fn reference_month(text: &str) -> Option<String> {
    let flat = Regex::new(r"\s+")
        .unwrap()
        .replace_all(text, " ")
        .to_string();
    let re = Regex::new(r"(?i)RETAIL AND FOOD SERVICES,? ([A-Z]+) (\d{4})").unwrap();
    let c = re.captures(&flat)?;
    let d = NaiveDate::parse_from_str(&format!("1 {} {}", &c[1], &c[2]), "%d %B %Y").ok()?;
    Some(d.format("%Y-%m").to_string())
}

pub fn events(releases: &[Release]) -> Result<Vec<Event>> {
    let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
    let mut events = Vec::new();
    let mut prev: Option<&Release> = None;
    for r in releases {
        let (y, m) = r.month.split_once('-').context("month")?;
        let label = format!("{y}年{}月分", m.trim_start_matches('0'));
        let mut facts = vec![
            Fact::text("reference", "対象月", label.clone()),
            Fact::signed(
                "retail_mom",
                "小売売上高 前月比（速報、季節調整済み）",
                r.mom,
                1,
                "%",
            ),
        ];
        if let Some(p) = prev {
            facts.push(Fact::signed(
                "retail_mom_prev",
                "前回発表の前月比（速報）",
                p.mom,
                1,
                "%",
            ));
        }
        let time_utc = ny
            .from_local_datetime(
                &r.released
                    .and_time(NaiveTime::from_hms_opt(8, 30, 0).unwrap()),
            )
            .earliest()
            .context("release time")?
            .with_timezone(&chrono::Utc);
        events.push(Event {
            id: format!("retail-{}", r.released),
            kind: "retail".into(),
            title: format!(
                "米小売売上高 {}（{label}）",
                r.released.format("%Y年%-m月%-d日")
            ),
            date: r.released,
            time_utc,
            time_approx: false,
            facts,
            sources: vec![SourceRef {
                label: "Census Bureau, Advance Monthly Sales for Retail and Food Services".into(),
                url: r.url.clone(),
            }],
        });
        prev = Some(r);
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
    fn parses_report_layouts() {
        let old = "FOR IMMEDIATE RELEASE\nFRIDAY, FEBRUARY 12, 2016, AT 8:30 A.M. EST\n... adjusted for seasonal variation, were $449.9 billion, an increase of 0.2 percent (±0.5%)* from the previous month";
        assert_eq!(parse_report(old), Some((d("2016-02-12"), 0.2)));
        let down = "FOR RELEASE AT 8:30 AM EST, FRIDAY, FEBRUARY 14, 2025\nwere $723.9 billion, down 0.9 percent (±0.5%)";
        assert_eq!(parse_report(down), Some((d("2025-02-14"), -0.9)));
        let flat = "FOR RELEASE AT 8:30 AM EDT, TUESDAY, JULY 16, 2024 were $704.3 billion, virtually unchanged (±0.5%)*";
        assert_eq!(parse_report(flat), Some((d("2024-07-16"), 0.0)));
        let table = "FOR RELEASE AT 8:30 AM EDT, WEDNESDAY, SEPTEMBER 16, 2026\nADVANCE MONTHLY SALES\n\nAugust 2026\n\nJuly 2026\n(revised)\n\n$773.9 billion\n\n$764.5 billion\n\n1.2%\n\n-0.5%";
        assert_eq!(parse_report(table), Some((d("2026-09-16"), 1.2)));
        let variants = "FOR RELEASE AT 8:30 AM EDT FRIDAY JULY 15, 2016 were $ 457.0 billion, an increase of 0.6 percent (±0.5%)";
        assert_eq!(parse_report(variants), Some((d("2016-07-15"), 0.6)));
        let pct =
            "THURSDAY, December 14, 2023 were $705.7 billion, a decrease of 1.3% (±0.5 percent)";
        assert_eq!(parse_report(pct), Some((d("2023-12-14"), -1.3)));
    }

    #[test]
    fn reads_reference_month() {
        let t = "FOR RELEASE AT 8:30 AM EDT, THURSDAY, OCTOBER 15, 2026\n\nADVANCE MONTHLY SALES FOR RETAIL AND FOOD SERVICES, SEPTEMBER 2026\n";
        assert_eq!(reference_month(t).as_deref(), Some("2026-09"));
        let old = "ADVANCE MONTHLY SALES FOR RETAIL AND FOOD SERVICES\nJANUARY 2016\n";
        assert_eq!(reference_month(old).as_deref(), Some("2016-01"));
    }

    #[test]
    fn lists_reports() {
        let html = r#"<a href="https://www2.census.gov/retail/releases/historical/marts/adv1703.pdf">PDF</a>
<a href="https://www2.census.gov/retail/releases/historical/marts/adv1703.pdf">again</a>"#;
        let l = report_links(html);
        assert_eq!(l.len(), 1);
        assert!(l["1703"].ends_with("adv1703.pdf"));
    }
}
