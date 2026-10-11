//! Bank of England MPC decisions: meeting pages from the site map (one page per announcement,
//! including holds), Bank Rate levels from the official Bank Rate history (changes only, effective
//! on the announcement day).

use crate::http;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveTime, TimeZone};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;

const FIRST_YEAR: i32 = 2016;
const SITEMAP: &str = "https://www.bankofengland.co.uk/_api/sitemap/getsitemap";
const BANK_RATE: &str = "https://www.bankofengland.co.uk/boeapps/database/Bank-Rate.asp";

/// Meeting page URLs, e.g. `/monetary-policy-summary-and-minutes/2024/august-2024`.
pub fn meeting_pages(sitemap: &str) -> Vec<String> {
    let re = Regex::new(
        r"<loc>\s*([^<]*/monetary-policy-summary-and-minutes/(20\d\d)/[^<\s]+)\s*</loc>",
    )
    .unwrap();
    let mut out: Vec<String> = re
        .captures_iter(sitemap)
        .filter(|c| c[2].parse::<i32>().is_ok_and(|y| y >= FIRST_YEAR))
        .map(|c| c[1].to_string())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// `(changed on, Bank Rate)` from the history table, oldest first.
pub fn parse_bank_rate(html: &str) -> Vec<(NaiveDate, f64)> {
    let re = Regex::new(r#"<td align="left">\s*(\d{2} \w{3} \d{2})\s*</td>\s*<td align="right">\s*([\d.]+)\s*</td>"#)
        .unwrap();
    let mut out: Vec<(NaiveDate, f64)> = re
        .captures_iter(html)
        .filter_map(|c| {
            Some((
                NaiveDate::parse_from_str(&c[1], "%d %b %y").ok()?,
                c[2].parse().ok()?,
            ))
        })
        .collect();
    out.sort_by_key(|(d, _)| *d);
    out
}

/// Publication date of a meeting page (`Published on 01 August 2024`); `None` for the
/// placeholders of future meetings, titled "… (to be published at 12pm)".
pub fn published_on(html: &str) -> Option<NaiveDate> {
    let title = Regex::new(r#"og:title"\s+content="([^"]*)""#).unwrap();
    if title
        .captures(html)
        .is_some_and(|c| c[1].to_lowercase().contains("to be published"))
    {
        return None;
    }
    let re = Regex::new(r"Published on\s+(\d{1,2} [A-Za-z]+ \d{4})").unwrap();
    NaiveDate::parse_from_str(&re.captures(&http::html_text(html))?[1], "%d %B %Y").ok()
}

/// The 11 March 2020 emergency cut was announced at 07:00 and its page is the minutes published
/// on the 13th; the special meeting of 19 March 2020 has no stated time either.
fn announcement(published: NaiveDate) -> (NaiveDate, NaiveTime, bool) {
    let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
    if published == NaiveDate::from_ymd_opt(2020, 3, 13).unwrap() {
        return (
            NaiveDate::from_ymd_opt(2020, 3, 11).unwrap(),
            NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            true,
        );
    }
    let special = published == NaiveDate::from_ymd_opt(2020, 3, 19).unwrap();
    (published, noon, special)
}

fn level_on(xs: &[(NaiveDate, f64)], date: NaiveDate) -> Option<f64> {
    xs.iter().rev().find(|(d, _)| *d <= date).map(|(_, v)| *v)
}

pub fn collect(client: &reqwest::blocking::Client, cache: &Path) -> Result<Vec<Event>> {
    std::fs::create_dir_all(cache)?;
    let rates = parse_bank_rate(&http::get_text(client, BANK_RATE)?);
    anyhow::ensure!(!rates.is_empty(), "no Bank Rate history");
    let pages = meeting_pages(&http::get_text(client, SITEMAP)?);
    anyhow::ensure!(!pages.is_empty(), "no MPC pages in the site map");
    let london: chrono_tz::Tz = "Europe/London".parse().unwrap();
    let today = chrono::Utc::now().date_naive();
    let mut by_date: BTreeMap<NaiveDate, (NaiveTime, bool, String)> = BTreeMap::new();
    for url in pages {
        let slug = url
            .rsplit("monetary-policy-summary-and-minutes/")
            .next()
            .unwrap_or(&url);
        let cached = cache.join(format!("{}.txt", slug.replace('/', "_")));
        let published = match std::fs::read_to_string(&cached)
            .ok()
            .and_then(|t| NaiveDate::parse_from_str(t.trim(), "%Y-%m-%d").ok())
        {
            Some(d) => Some(d),
            None => {
                std::thread::sleep(std::time::Duration::from_millis(300));
                match http::get_text(client, &url) {
                    Ok(html) => {
                        let d = published_on(&html);
                        // Only real announcements are cached; placeholders are fetched again later.
                        if let Some(d) = d.filter(|d| *d <= today) {
                            std::fs::write(&cached, d.to_string())?;
                        }
                        d
                    }
                    Err(e) => {
                        tracing::warn!("boe {url}: {e:#}");
                        None
                    }
                }
            }
        };
        let Some(published) = published.filter(|d| *d <= today) else {
            continue;
        };
        let (date, time, approx) = announcement(published);
        by_date.entry(date).or_insert((time, approx, url));
    }
    let mut events = Vec::new();
    for (date, (time, approx, url)) in by_date {
        let (Some(before), Some(after)) = (
            level_on(&rates, date.pred_opt().unwrap()),
            level_on(&rates, date),
        ) else {
            tracing::warn!("boe {date}: no Bank Rate");
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
        let time_utc = london
            .from_local_datetime(&date.and_time(time))
            .earliest()
            .context("announcement time")?
            .with_timezone(&chrono::Utc);
        events.push(Event {
            id: format!("boe-{date}"),
            kind: "boe".into(),
            title: format!(
                "BoE 金融政策委員会 {}（{decision}）",
                date.format("%Y年%-m月%-d日")
            ),
            date,
            time_utc,
            time_approx: approx,
            facts: vec![
                Fact::text("decision", "決定", decision),
                Fact::text("bank_rate", "政策金利（Bank Rate）", format!("{after:.2}%")),
                Fact::signed("change_bp", "変更幅", change_bp, 0, "bp"),
                Fact::text("prev_rate", "変更前", format!("{before:.2}%")),
            ],
            sources: vec![
                SourceRef {
                    label: "Monetary Policy Summary".into(),
                    url,
                },
                SourceRef {
                    label: "Bank Rate history".into(),
                    url: BANK_RATE.into(),
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
    fn parses_bank_rate_history() {
        let html = r#"<tr><td align="left">18 Dec 25</td>
            <td align="right">  3.75</td></tr><tr><td align="left">07 Aug 25</td><td align="right">  4.00</td></tr>"#;
        let r = parse_bank_rate(html);
        assert_eq!(r, vec![(d("2025-08-07"), 4.0), (d("2025-12-18"), 3.75)]);
        assert_eq!(level_on(&r, d("2025-12-17")), Some(4.0));
        assert_eq!(level_on(&r, d("2025-12-18")), Some(3.75));
    }

    #[test]
    fn reads_publication_dates_and_skips_placeholders() {
        let page = r#"<meta property="og:title" content="Bank Rate reduced to 5% - August 2024" />
<div class="published-date">
    Published on  
        01 August 2024
</div>"#;
        assert_eq!(published_on(page), Some(d("2024-08-01")));
        let placeholder = r#"<meta property="og:title" content="Monetary Policy Summary, November 2026 (to be published at 12pm)" />
<div class="published-date">Published on 05 November 2026</div>"#;
        assert_eq!(published_on(placeholder), None);
    }

    #[test]
    fn filters_site_map() {
        let xml = "<url><loc>https://www.bankofengland.co.uk/monetary-policy-summary-and-minutes/2015/december-2015</loc></url>\
<url><loc>https://www.bankofengland.co.uk/monetary-policy-summary-and-minutes/2024/august-2024</loc></url>\
<url><loc>https://www.bankofengland.co.uk/monetary-policy-summary-and-minutes/monetary-policy-summary-and-minutes</loc></url>";
        assert_eq!(
            meeting_pages(xml),
            vec!["https://www.bankofengland.co.uk/monetary-policy-summary-and-minutes/2024/august-2024"]
        );
    }

    #[test]
    fn emergency_cut_is_dated_on_announcement() {
        let (date, time, approx) = announcement(d("2020-03-13"));
        assert_eq!(
            (date, time, approx),
            (
                d("2020-03-11"),
                NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
                true
            )
        );
    }
}
