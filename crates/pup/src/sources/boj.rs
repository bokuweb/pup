//! Bank of Japan Monetary Policy Meetings. The meeting's last day is taken from the "Summary of
//! Opinions" documents (`opiYYMMDD.pdf`, or `.htm` in older years), which exist for every meeting
//! since 2016 and are published about a week later. A meeting held since then is taken from the
//! schedule page instead, which links its statement (`kYYMMDDa.pdf`, named after the last day) on
//! the day. The decision itself is not parsed yet (it is only in PDFs), so the facts are the date.

use crate::http;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, NaiveTime, TimeZone};
use regex::Regex;
use std::collections::BTreeSet;

const BASE: &str = "https://www.boj.or.jp";
const FIRST_YEAR: i32 = 2016;

pub fn meeting_dates(client: &reqwest::blocking::Client) -> Result<Vec<(NaiveDate, String)>> {
    let re = Regex::new(r"(/mopo/mpmsche_minu/opinion_\d{4}/)?opi(\d{6})\.(pdf|htm)").unwrap();
    let mut out = BTreeSet::new();
    let this_year = chrono::Utc::now().year();
    for y in FIRST_YEAR..=this_year {
        let url = format!("{BASE}/mopo/mpmsche_minu/opinion_{y}/index.htm");
        match http::get_text(client, &url) {
            Ok(html) => {
                for c in re.captures_iter(&html) {
                    if let Ok(d) = NaiveDate::parse_from_str(&format!("20{}", &c[2]), "%Y%m%d") {
                        out.insert((
                            d,
                            format!(
                                "{BASE}/mopo/mpmsche_minu/opinion_{y}/opi{}.{}",
                                &c[2], &c[3]
                            ),
                        ));
                    }
                }
            }
            Err(e) => tracing::warn!("boj {url}: {e:#}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    // A meeting can be linked as both PDF and HTML; keep one URL per date.
    let mut seen = BTreeSet::new();
    Ok(out.into_iter().filter(|(d, _)| seen.insert(*d)).collect())
}

/// `(last day, statement URL)` of the meetings held so far, from the schedule page (this year and
/// the next; a meeting not yet held has no statement link).
pub fn statement_links(html: &str) -> Vec<(NaiveDate, String)> {
    let re = Regex::new(r#"href="(/mopo/mpmdeci/mpr_\d{4}/k(\d{6})[a-z]?\.pdf)""#).unwrap();
    let mut seen = BTreeSet::new();
    re.captures_iter(html)
        .filter_map(|c| {
            let d = NaiveDate::parse_from_str(&format!("20{}", &c[2]), "%Y%m%d").ok()?;
            seen.insert(d).then(|| (d, format!("{BASE}{}", &c[1])))
        })
        .collect()
}

pub fn collect(client: &reqwest::blocking::Client) -> Result<Vec<Event>> {
    let tokyo: chrono_tz::Tz = "Asia/Tokyo".parse().unwrap();
    let mut meetings: Vec<(NaiveDate, String, &str)> = meeting_dates(client)?
        .into_iter()
        .map(|(d, url)| (d, url, "金融政策決定会合における主な意見"))
        .collect();
    match http::get_text(client, &format!("{BASE}/mopo/mpmsche_minu/index.htm")) {
        Ok(html) => {
            for (d, url) in statement_links(&html) {
                if !meetings.iter().any(|m| m.0 == d) {
                    meetings.push((d, url, "当面の金融政策運営について（公表文）"));
                }
            }
        }
        Err(e) => tracing::warn!("boj schedule: {e:#}"),
    }
    meetings.sort_by_key(|m| m.0);
    meetings
        .into_iter()
        .map(|(date, url, label)| {
            // Decisions are released around midday Tokyo time but the exact minute varies.
            let time_utc = tokyo
                .from_local_datetime(&date.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap()))
                .earliest()
                .context("time")?
                .with_timezone(&chrono::Utc);
            Ok(Event {
                id: format!("boj-{date}"),
                kind: "boj".into(),
                title: format!("日銀金融政策決定会合 {}", date.format("%Y年%-m月%-d日")),
                date,
                time_utc,
                time_approx: true,
                facts: vec![Fact::text("meeting_end", "会合最終日", date.to_string())],
                sources: vec![SourceRef {
                    label: label.into(),
                    url,
                }],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_statement_links() {
        let html = r#"<td><a href="/mopo/mpmdeci/mpr_2026/k260918a.pdf">9月17日（木）・18日（金） [PDF 220KB]</a></td>
<td><a href="/mopo/mpmdeci/mpr_2026/k261030a.pdf">10月29日（木）・30日（金） [PDF 153KB]</a></td>
<td><a href="/mopo/outlook/gor2610a.pdf">10月30日（金）</a></td>
<td>12月17日（木）・18日（金）</td>"#;
        let got: Vec<String> = statement_links(html)
            .iter()
            .map(|(d, u)| format!("{d} {u}"))
            .collect();
        assert_eq!(
            got,
            vec![
                "2026-09-18 https://www.boj.or.jp/mopo/mpmdeci/mpr_2026/k260918a.pdf",
                "2026-10-30 https://www.boj.or.jp/mopo/mpmdeci/mpr_2026/k261030a.pdf",
            ]
        );
    }
}
