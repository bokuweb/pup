//! BLS release dates without a FRED key. bls.gov refuses scripted clients, but every release is
//! archived as `news.release/archives/{name}_{MMDDYYYY}.htm`, and the Internet Archive's CDX index
//! of those URLs gives the dates. A release shows up once the Archive has crawled it, which can lag
//! the release by days or weeks, so the latest releases are taken from the BLS monthly release
//! calendar (`schedule/{year}/{MM}_sched.htm`, also read through the Archive) once BLS data for
//! their reference month exist.

use crate::http;
use anyhow::Result;
use chrono::NaiveDate;
use regex::Regex;
use std::collections::BTreeSet;

/// `cpi`, `empsit`, `ppi`, `jolts`.
pub fn bls_release_dates(
    client: &reqwest::blocking::Client,
    name: &str,
    from: NaiveDate,
) -> Result<Vec<NaiveDate>> {
    let url = format!(
        "https://web.archive.org/cdx/search/cdx?url=bls.gov/news.release/archives/{name}_&matchType=prefix&collapse=urlkey&fl=original&filter=statuscode:200"
    );
    let dates = parse_cdx(&http::get_text(client, &url)?, name);
    anyhow::ensure!(!dates.is_empty(), "no archived {name} releases");
    Ok(dates.into_iter().filter(|d| *d >= from).collect())
}

/// One scheduled release: calendar title, release date, reference month.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheduled {
    pub title: String,
    pub date: NaiveDate,
    pub reference: (i32, u32),
}

/// The BLS release calendars of the given months, from their latest Archive snapshots.
pub fn bls_schedule(client: &reqwest::blocking::Client, months: &[(i32, u32)]) -> Vec<Scheduled> {
    let mut out = Vec::new();
    for &(y, m) in months {
        let url = format!(
            "https://web.archive.org/web/{y}id_/https://www.bls.gov/schedule/{y}/{m:02}_sched.htm"
        );
        match http::get_text_gunzip(client, &url) {
            Ok(html) => out.extend(parse_month_schedule(&html, y, m)),
            Err(e) => tracing::warn!("bls schedule {y}-{m:02}: {e:#}"),
        }
    }
    out
}

/// A calendar page is a grid of day numbers, each followed by `title | reference period | time`.
/// Days before the 1st belong to the previous month and are skipped, as are days after the last
/// one (the next month).
pub fn parse_month_schedule(html: &str, year: i32, month: u32) -> Vec<Scheduled> {
    let tag = Regex::new(r"<[^>]+>").unwrap();
    let tokens: Vec<String> = tag
        .split(html)
        .map(http::html_text)
        .filter(|t| !t.is_empty())
        .collect();
    let period = Regex::new(
        r"^(January|February|March|April|May|June|July|August|September|October|November|December) (\d{4})$",
    )
    .unwrap();
    let time = Regex::new(r"^\d{2}:\d{2} [AP]M$").unwrap();
    let mut out = Vec::new();
    let (mut day, mut started, mut last) = (0u32, false, 0u32);
    for (i, t) in tokens.iter().enumerate() {
        if let Ok(d) = t.parse::<u32>() {
            if (1..=31).contains(&d) {
                if d == 1 {
                    if started {
                        break; // the next month's leading days
                    }
                    started = true;
                    last = 0;
                }
                if started && d < last {
                    break;
                }
                day = d;
                last = d;
                continue;
            }
        }
        let (Some(p), Some(tm)) = (tokens.get(i + 1), tokens.get(i + 2)) else {
            continue;
        };
        let (Some(c), true) = (period.captures(p), time.is_match(tm)) else {
            continue;
        };
        if !started {
            continue;
        }
        let Ok(r) =
            chrono::NaiveDate::parse_from_str(&format!("1 {} {}", &c[1], &c[2]), "%d %B %Y")
        else {
            continue;
        };
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
            out.push(Scheduled {
                title: t.clone(),
                date,
                reference: (chrono::Datelike::year(&r), chrono::Datelike::month(&r)),
            });
        }
    }
    out
}

pub fn parse_cdx(body: &str, name: &str) -> BTreeSet<NaiveDate> {
    let re = Regex::new(&format!(
        r"news\.release/archives/{}_(\d{{2}})(\d{{2}})(\d{{4}})\.(?:htm|pdf)",
        regex::escape(name)
    ))
    .unwrap();
    re.captures_iter(body)
        .filter_map(|c| {
            NaiveDate::from_ymd_opt(c[3].parse().ok()?, c[1].parse().ok()?, c[2].parse().ok()?)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_monthly_calendar() {
        let html = "<tr><td>28</td><td>29</td><td><p>Job Openings and Labor Turnover Survey</p><p>August 2026</p><p>10:00 AM</p></td>\
<td>1</td><td>2</td><td><p>Employment Situation</p><p>September 2026</p><p>08:30 AM</p></td>\
<td>12</td><td>Columbus Day</td><td>Holiday</td><td>14</td><td><p>Consumer Price Index</p><p>September 2026</p><p>08:30 AM</p>\
<p>Real Earnings</p><p>September 2026</p><p>08:30 AM</p></td><td>30</td><td>1</td><td><p>Producer Price Index</p><p>October 2026</p><p>08:30 AM</p></td></tr>";
        let got = parse_month_schedule(html, 2026, 10);
        let names: Vec<(String, String, (i32, u32))> = got
            .iter()
            .map(|s| (s.title.clone(), s.date.to_string(), s.reference))
            .collect();
        assert_eq!(
            names,
            vec![
                (
                    "Employment Situation".into(),
                    "2026-10-02".into(),
                    (2026, 9)
                ),
                (
                    "Consumer Price Index".into(),
                    "2026-10-14".into(),
                    (2026, 9)
                ),
                ("Real Earnings".into(), "2026-10-14".into(), (2026, 9)),
            ]
        );
    }

    #[test]
    fn reads_dates_from_archive_names() {
        let body = "http://www.bls.gov/news.release/archives/cpi_01112024.htm\n\
https://www.bls.gov/news.release/archives/cpi_01112024.pdf\n\
http://bls.gov:80/news.release/archives/cpi_10242025.htm?foo=1\n\
http://www.bls.gov/news.release/archives/cpi_nr0.htm\n\
http://www.bls.gov/news.release/archives/cpi_13452024.htm\n";
        let got: Vec<String> = parse_cdx(body, "cpi")
            .iter()
            .map(|d| d.to_string())
            .collect();
        assert_eq!(got, vec!["2024-01-11", "2025-10-24"]);
    }
}
