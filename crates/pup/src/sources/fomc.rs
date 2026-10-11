//! FOMC statements from federalreserve.gov: dates from the meeting calendars, decisions from each statement.

use crate::http;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveTime, TimeZone};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;

const BASE: &str = "https://www.federalreserve.gov";
const FIRST_HISTORICAL_YEAR: i32 = 2015;

/// Statement dates from the current calendar page plus the historical pages back to 2015.
pub fn statement_dates(client: &reqwest::blocking::Client) -> Result<Vec<NaiveDate>> {
    let link = Regex::new(r"/newsevents/pressreleases/monetary(\d{8})a\.htm").unwrap();
    let mut dates = BTreeSet::new();
    let mut pages = vec![format!("{BASE}/monetarypolicy/fomccalendars.htm")];
    let current = chrono::Utc::now().year_ce().1 as i32;
    for y in FIRST_HISTORICAL_YEAR..=current - 6 {
        pages.push(format!("{BASE}/monetarypolicy/fomchistorical{y}.htm"));
    }
    for url in pages {
        match http::get_text(client, &url) {
            Ok(html) => {
                for c in link.captures_iter(&html) {
                    if let Ok(d) = NaiveDate::parse_from_str(&c[1], "%Y%m%d") {
                        dates.insert(d);
                    }
                }
            }
            Err(e) => tracing::warn!("fomc calendar {url}: {e:#}"),
        }
    }
    Ok(dates
        .into_iter()
        .filter(|d| d.year_ce().1 as i32 >= FIRST_HISTORICAL_YEAR)
        .collect())
}

use chrono::Datelike;

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub action: &'static str,
    pub lower: f64,
    pub upper: f64,
    pub release: NaiveTime,
}

/// `4-1/4` → 4.25, `1/4` → 0.25, `5` → 5.0.
pub fn parse_rate(s: &str) -> Option<f64> {
    let s = s.trim().replace(['\u{2011}', '\u{2013}'], "-");
    let (whole, frac) = match s.split_once('-') {
        Some((w, f)) => (w.trim(), Some(f.trim())),
        None if s.contains('/') => ("0", Some(s.as_str())),
        None => (s.as_str(), None),
    };
    let mut v: f64 = whole.parse().ok()?;
    if let Some(f) = frac {
        let (n, d) = f.split_once('/')?;
        v += n.trim().parse::<f64>().ok()? / d.trim().parse::<f64>().ok()?;
    }
    Some(v)
}

pub fn parse_statement(text: &str) -> Option<Decision> {
    // "decided (today) to lower the target range ... by 1/4 percentage point(,) to 4 to 4-1/4 percent"
    // "decided to keep/maintain the target range ... at 0 to 1/4 percent"
    let decided = Regex::new(
        r"(?i)decided(?: today)? to (raise|lower|maintain|keep|reduce|increase)\s+the target range for the federal funds rate[^.]*?(?:at|to)\s+([0-9][0-9/\-]*)\s+to\s+([0-9][0-9/\-]*)\s+percent",
    )
    .unwrap();
    // Zero-rate era: "the current 0 to 1/4 percent target range for the federal funds rate remains appropriate"
    let remains = Regex::new(r"(?i)current\s+([0-9][0-9/\-]*)\s+to\s+([0-9][0-9/\-]*)\s+percent target range for the federal funds rate remains appropriate").unwrap();
    let (verb, lo, hi) = if let Some(c) = decided.captures(text) {
        (c[1].to_lowercase(), c[2].to_string(), c[3].to_string())
    } else {
        let c = remains.captures(text)?;
        ("maintain".to_string(), c[1].to_string(), c[2].to_string())
    };
    let action = match verb.as_str() {
        "raise" | "increase" => "hike",
        "lower" | "reduce" => "cut",
        _ => "hold",
    };
    let time = Regex::new(r"(?i)release at (\d{1,2}):(\d{2})\s*([ap])\.?m\.?").unwrap();
    let release = time
        .captures(text)
        .and_then(|t| {
            let h: u32 = t[1].parse().ok()?;
            let h = match (&t[3].to_lowercase()[..], h) {
                ("p", 12) => 12,
                ("p", h) => h + 12,
                ("a", 12) => 0,
                (_, h) => h,
            };
            NaiveTime::from_hms_opt(h, t[2].parse().ok()?, 0)
        })
        .unwrap_or_else(|| NaiveTime::from_hms_opt(14, 0, 0).unwrap());
    Some(Decision {
        action,
        lower: parse_rate(&lo)?,
        upper: parse_rate(&hi)?,
        release,
    })
}

pub fn collect(client: &reqwest::blocking::Client, cache: &Path) -> Result<Vec<Event>> {
    std::fs::create_dir_all(cache)?;
    let mut events = Vec::new();
    let mut prev: Option<Decision> = None;
    for date in statement_dates(client)? {
        let url = format!(
            "{BASE}/newsevents/pressreleases/monetary{}a.htm",
            date.format("%Y%m%d")
        );
        let cached = cache.join(format!("{date}.txt"));
        let text = match std::fs::read_to_string(&cached) {
            Ok(t) => t,
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let t = http::html_text(&http::get_text(client, &url)?);
                std::fs::write(&cached, &t)?;
                t
            }
        };
        let Some(d) = parse_statement(&text) else {
            tracing::warn!("fomc {date}: no rate decision found in statement");
            continue;
        };
        let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
        let time_utc = ny
            .from_local_datetime(&date.and_time(d.release))
            .earliest()
            .context("release time")?
            .with_timezone(&chrono::Utc);
        let change_bp = prev.as_ref().map(|p| (d.upper - p.upper) * 100.0);
        let mut facts = vec![
            Fact::text(
                "decision",
                "決定",
                match d.action {
                    "hike" => "利上げ",
                    "cut" => "利下げ",
                    _ => "据え置き",
                },
            ),
            Fact::text(
                "target_range",
                "政策金利（FF 金利誘導目標）",
                format!("{:.2}〜{:.2}%", d.lower, d.upper),
            ),
        ];
        if let Some(bp) = change_bp {
            facts.push(Fact::signed("change_bp", "変更幅", bp, 0, "bp"));
        }
        if let Some(p) = &prev {
            facts.push(Fact::text(
                "prev_range",
                "変更前",
                format!("{:.2}〜{:.2}%", p.lower, p.upper),
            ));
        }
        events.push(Event {
            id: format!("fomc-{date}"),
            kind: "fomc".into(),
            title: format!(
                "FOMC {}（{}）",
                date.format("%Y年%-m月%-d日"),
                facts[0].text
            ),
            date,
            time_utc,
            time_approx: false,
            facts,
            sources: vec![SourceRef {
                label: "FOMC statement".into(),
                url,
            }],
        });
        prev = Some(d);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fractions() {
        assert_eq!(parse_rate("4-1/4"), Some(4.25));
        assert_eq!(parse_rate("4\u{2011}1/2"), Some(4.5));
        assert_eq!(parse_rate("1/4"), Some(0.25));
        assert_eq!(parse_rate("0"), Some(0.0));
    }

    #[test]
    fn parses_statements() {
        let cut = "For release at 2:00 p.m. EDT ... the Committee decided to lower the target range for the federal funds rate by 1/4 percentage point to 4 to 4-1/4 percent. In considering";
        assert_eq!(
            parse_statement(cut),
            Some(Decision {
                action: "cut",
                lower: 4.0,
                upper: 4.25,
                release: NaiveTime::from_hms_opt(14, 0, 0).unwrap()
            })
        );
        let hold = "decided to maintain the target range for the federal funds rate at 4-1/4 to 4-1/2 percent.";
        assert_eq!(parse_statement(hold).unwrap().action, "hold");
        let keep = "the Committee decided to keep the target range for the federal funds rate at 0 to 1/4 percent and expects";
        assert_eq!(parse_statement(keep).unwrap().action, "hold");
        let today = "the Committee decided today to lower the target range for the federal funds rate by 1/2 percentage point, to 1 to 1-1/4 percent.";
        assert_eq!(parse_statement(today).unwrap().upper, 1.25);
        let zirp = "the Committee today reaffirmed its view that the current 0 to 1/4 percent target range for the federal funds rate remains appropriate.";
        assert_eq!(parse_statement(zirp).unwrap().upper, 0.25);
        let emergency = "For release at 5:00 p.m. EDT the Committee decided to lower the target range for the federal funds rate to 0 to 1/4 percent.";
        let d = parse_statement(emergency).unwrap();
        assert_eq!(
            (d.lower, d.upper, d.release),
            (0.0, 0.25, NaiveTime::from_hms_opt(17, 0, 0).unwrap())
        );
    }
}
