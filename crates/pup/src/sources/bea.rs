//! US GDP advance estimates and the PCE price index (BEA): release lists from the bea.gov news
//! archive by product, headline figures as first published from each release page.
//! The archive lags the latest releases by weeks, so the current-releases page is read too.

use crate::http;
use crate::model::{Event, Fact, SourceRef};
use anyhow::Result;
use chrono::{DateTime, FixedOffset, NaiveDate, Timelike};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;

const BASE: &str = "https://www.bea.gov";
/// Archive product ids: GDP; PCE price index and Personal Income (the PIO releases are tagged
/// inconsistently, so both are read).
const GDP: &[u32] = &[451];
const PIO: &[u32] = &[716, 476];

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub title: String,
    pub path: String,
    pub time: DateTime<FixedOffset>,
}

/// Rows of one archive page. The page is split on `release-row` so a navigation link before the
/// table is never taken for a release.
pub fn archive_rows(html: &str) -> Vec<Row> {
    let link = Regex::new(r#"<a href="(/news/\d{4}/[^"]+)"[^>]*>([^<]+)</a>"#).unwrap();
    let time = Regex::new(r#"<time datetime="([^"]+)""#).unwrap();
    html.split("release-row")
        .skip(1)
        .filter_map(|chunk| {
            let l = link.captures(chunk)?;
            let t = DateTime::parse_from_rfc3339(&time.captures(chunk)?[1]).ok()?;
            Some(Row {
                title: http::html_text(&l[2]),
                path: l[1].to_string(),
                time: t,
            })
        })
        .collect()
}

/// Every archived release of the products since `since`, newest pages first until older.
fn archive(
    client: &reqwest::blocking::Client,
    products: &[u32],
    since: NaiveDate,
) -> Result<Vec<Row>> {
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for id in products {
        for page in 0..40 {
            let url =
                format!("{BASE}/news/archive?field_related_product_target_id={id}&page={page}");
            let got = archive_rows(&http::get_text(client, &url)?);
            let oldest = got.iter().map(|r| r.time.date_naive()).min();
            for r in got {
                rows.entry(r.path.clone()).or_insert(r);
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
            if oldest.is_none_or(|d| d < since) {
                break;
            }
        }
    }
    // The archive lags the latest releases by weeks; the current-releases page has them at once.
    match http::get_text(client, &format!("{BASE}/news/current-releases")) {
        Ok(html) => {
            for r in archive_rows(&html) {
                rows.entry(r.path.clone()).or_insert(r);
            }
        }
        Err(e) => tracing::warn!("bea current releases: {e:#}"),
    }
    anyhow::ensure!(!rows.is_empty(), "empty BEA archive");
    Ok(rows
        .into_values()
        .filter(|r| r.time.date_naive() >= since)
        .collect())
}

/// Scheduled time: the archive stamps some releases a minute early or late (08:29:59, 08:31).
fn scheduled(t: DateTime<FixedOffset>) -> DateTime<chrono::Utc> {
    let mins = (t.hour() * 60 + t.minute()) as i64 + if t.second() >= 30 { 1 } else { 0 };
    let rounded = (mins + 15) / 30 * 30;
    let midnight = t.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let local = midnight + chrono::Duration::minutes(rounded);
    (local - chrono::Duration::seconds(t.offset().local_minus_utc() as i64)).and_utc()
}

fn page_text(client: &reqwest::blocking::Client, cache: &Path, path: &str) -> Result<String> {
    let cached = cache.join(format!(
        "{}.txt",
        path.trim_start_matches('/').replace('/', "_")
    ));
    if let Ok(t) = std::fs::read_to_string(&cached) {
        return Ok(t);
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    let t = http::html_text(&http::get_text(client, &format!("{BASE}{path}"))?);
    std::fs::write(&cached, &t)?;
    Ok(t)
}

fn signed(dir: &str, v: f64) -> f64 {
    if dir == "decreased" {
        -v
    } else {
        v
    }
}

/// `Real gross domestic product (GDP) increased at an annual rate of 1.5 percent` (2016 releases
/// insert a definition between the subject and the verb).
pub fn parse_gdp(text: &str) -> Option<f64> {
    let re = Regex::new(
        r"Real gross domestic product.{0,300}?(increased|decreased) at an annual rate of ([\d.]+) percent",
    )
    .unwrap();
    let c = re.captures(text)?;
    Some(signed(&c[1], c[2].parse().ok()?))
}

/// Headline and core PCE price index, month on month. Wordings since 2016: `The PCE price index
/// (for August) (also) increased 0.3 percent`, `The price index for PCE increased 0.1 percent`,
/// `Excluding food and energy, the PCE price index increased 0.2 percent`, `The PCE price index,
/// excluding food and energy, increased 0.3 percent`, `The PCE price index excluding food and
/// energy was also unchanged`; `less than 0.1 percent`, `was unchanged` and `had no change` are 0.0. The
/// first match is the monthly change (the y/y sentences come later).
pub fn parse_pce(text: &str) -> Option<(f64, f64)> {
    let tail = r" (?:also )?(?:(increased|decreased) (less than )?([\d.]+) percent|(was (?:also )?unchanged|had no change))";
    let head = Regex::new(&format!(
        r"(?:PCE price index(?: for \w+)?|price index for PCE){tail}"
    ))
    .unwrap();
    let core = Regex::new(&format!(
        r"(?:Excluding food and energy, the PCE price index|PCE price index,? excluding food and energy,?){tail}"
    ))
    .unwrap();
    let val = |c: regex::Captures| -> Option<f64> {
        match (c.get(1), c.get(2), c.get(3)) {
            (Some(_), Some(_), _) => Some(0.0),
            (Some(d), None, Some(v)) => Some(signed(d.as_str(), v.as_str().parse().ok()?)),
            _ => Some(0.0),
        }
    };
    Some((val(head.captures(text)?)?, val(core.captures(text)?)?))
}

fn quarter_label(title: &str) -> Option<String> {
    // `2nd Quarter 2026`, `second quarter 2022`, `4th quarter and annual 2015`,
    // `Fourth Quarter and Year 2019`, `Initial ... 4th quarter and annual 2018`
    let re = Regex::new(
        r"(?i)(1st|2nd|3rd|4th|first|second|third|fourth) quarter(?:,| and annual| and year)? (\d{4})",
    )
    .unwrap();
    let c = re.captures(title)?;
    let q = match c[1].to_lowercase().as_str() {
        "1st" | "first" => 1,
        "2nd" | "second" => 2,
        "3rd" | "third" => 3,
        _ => 4,
    };
    Some(format!("{}年第{q}四半期", &c[2]))
}

fn month_label(title: &str) -> Option<String> {
    let re = Regex::new(r"(January|February|March|April|May|June|July|August|September|October|November|December) (\d{4})").unwrap();
    let c = re.captures(title)?;
    let d = NaiveDate::parse_from_str(&format!("1 {} {}", &c[1], &c[2]), "%d %B %Y").ok()?;
    Some(d.format("%Y年%-m月分").to_string())
}

pub fn gdp_events(
    client: &reqwest::blocking::Client,
    cache: &Path,
    since: NaiveDate,
) -> Result<Vec<Event>> {
    std::fs::create_dir_all(cache)?;
    let advance = Regex::new(r"(?i)advance|initial").unwrap();
    let mut rows: Vec<Row> = archive(client, GDP, since)?
        .into_iter()
        .filter(|r| advance.is_match(&r.title))
        .collect();
    rows.sort_by_key(|r| r.time);
    let mut events = Vec::new();
    let mut prev: Option<f64> = None;
    for r in rows {
        let (Some(q), Ok(text)) = (quarter_label(&r.title), page_text(client, cache, &r.path))
        else {
            tracing::warn!("gdp {}: no quarter or page", r.path);
            continue;
        };
        let Some(v) = parse_gdp(&text) else {
            tracing::warn!("gdp {}: no headline", r.path);
            continue;
        };
        let mut facts = vec![
            Fact::text("quarter", "対象期間", q.clone()),
            Fact::signed("gdp_saar", "実質GDP 前期比年率（速報）", v, 1, "%"),
        ];
        if let Some(p) = prev {
            facts.push(Fact::signed("gdp_saar_prev", "前期の速報値", p, 1, "%"));
        }
        let date = r.time.date_naive();
        events.push(Event {
            id: format!("gdp-{date}"),
            kind: "gdp".into(),
            title: format!("米GDP速報 {}（{q}）", date.format("%Y年%-m月%-d日")),
            date,
            time_utc: scheduled(r.time),
            time_approx: false,
            facts,
            sources: vec![SourceRef {
                label: format!("BEA: {}", r.title),
                url: format!("{BASE}{}", r.path),
            }],
        });
        prev = Some(v);
    }
    Ok(events)
}

pub fn pce_events(
    client: &reqwest::blocking::Client,
    cache: &Path,
    since: NaiveDate,
) -> Result<Vec<Event>> {
    std::fs::create_dir_all(cache)?;
    let pio = Regex::new(r"(?i)^Personal Income and Outlays").unwrap();
    let mut rows: Vec<Row> = archive(client, PIO, since)?
        .into_iter()
        .filter(|r| pio.is_match(&r.title))
        .collect();
    rows.sort_by_key(|r| r.time);
    // One release per day (a few are listed twice under different titles).
    rows.dedup_by_key(|r| r.time.date_naive());
    let mut events = Vec::new();
    let mut prev: Option<f64> = None;
    for r in rows {
        let (Some(m), Ok(text)) = (month_label(&r.title), page_text(client, cache, &r.path)) else {
            tracing::warn!("pce {}: no month or page", r.path);
            continue;
        };
        let Some((head, core)) = parse_pce(&text) else {
            tracing::warn!("pce {}: no headline", r.path);
            continue;
        };
        let mut facts = vec![
            Fact::text("reference", "対象月", m.clone()),
            Fact::signed("pce_mom", "PCE 物価指数 前月比", head, 1, "%"),
            Fact::signed("core_pce_mom", "コア PCE 物価指数 前月比", core, 1, "%"),
        ];
        if let Some(p) = prev {
            facts.push(Fact::signed(
                "core_pce_mom_prev",
                "前回発表のコア前月比",
                p,
                1,
                "%",
            ));
        }
        let date = r.time.date_naive();
        events.push(Event {
            id: format!("pce-{date}"),
            kind: "pce".into(),
            title: format!("米PCE物価 {}（{m}）", date.format("%Y年%-m月%-d日")),
            date,
            time_utc: scheduled(r.time),
            time_approx: false,
            facts,
            sources: vec![SourceRef {
                label: format!("BEA: {}", r.title),
                url: format!("{BASE}{}", r.path),
            }],
        });
        prev = Some(core);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_archive_rows() {
        let html = r#"<a href="/news/2026/glance">Economy at a Glance</a><table><tr class="release-row">
<td><a href="/news/2026/gdp-advance-estimate-2nd-quarter-2026" hreflang="en">GDP (Advance Estimate), 2nd Quarter 2026</a></td>
<td><time datetime="2026-07-30T08:31:00-04:00">July 30, 2026</time></td></tr>"#;
        let rows = archive_rows(html);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].path,
            "/news/2026/gdp-advance-estimate-2nd-quarter-2026"
        );
        assert_eq!(
            scheduled(rows[0].time).to_rfc3339(),
            "2026-07-30T12:30:00+00:00"
        );
        assert_eq!(
            quarter_label(&rows[0].title).as_deref(),
            Some("2026年第2四半期")
        );
        let t = DateTime::parse_from_rfc3339("2025-12-05T09:59:59-05:00").unwrap();
        assert_eq!(scheduled(t).to_rfc3339(), "2025-12-05T15:00:00+00:00");
        for (title, want) in [
            (
                "Gross Domestic Product, 4th quarter and annual 2015 (advance estimate)",
                "2015年第4四半期",
            ),
            (
                "Gross Domestic Product, Fourth Quarter and Year 2019 (Advance Estimate)",
                "2019年第4四半期",
            ),
            (
                "Gross Domestic Product, second quarter 2022 (advance estimate)",
                "2022年第2四半期",
            ),
        ] {
            assert_eq!(quarter_label(title).as_deref(), Some(want), "{title}");
        }
    }

    #[test]
    fn parses_pce_wordings() {
        let y2016 = "The price index for PCE increased 0.1 percent in January, in contrast to a decrease. The PCE price index, excluding food and energy, increased 0.3 percent, compared with";
        assert_eq!(parse_pce(y2016), Some((0.1, 0.3)));
        let also = "The PCE price index for July increased 0.2 percent. Excluding food and energy, the PCE price index also increased 0.2 percent. The PCE price index for July increased 3.7 percent. Excluding food and energy, the PCE price index increased 3.3 percent from one year ago";
        assert_eq!(parse_pce(also), Some((0.2, 0.2)));
        let tiny = "The PCE price index decreased less than 0.1 percent. Excluding food and energy, the PCE price index increased less than 0.1 percent";
        assert_eq!(parse_pce(tiny), Some((0.0, 0.0)));
        let unchanged = "The PCE price index was unchanged from September. The PCE price index excluding food and energy was also unchanged (table 9).";
        assert_eq!(parse_pce(unchanged), Some((0.0, 0.0)));
        let none = "The PCE price index had no change. Excluding food and energy, the PCE price index had no change (table 9).";
        assert_eq!(parse_pce(none), Some((0.0, 0.0)));
    }

    #[test]
    fn parses_headlines() {
        let old = "Real gross domestic product -- the value of the goods and services produced by the nation's economy less the value of the goods and services used up in production, adjusted for price changes -- increased at an annual rate of 0.5 percent in the first quarter";
        assert_eq!(parse_gdp(old), Some(0.5));
        assert_eq!(
            parse_gdp(
                "Real gross domestic product (GDP) decreased at an annual rate of 32.9 percent"
            ),
            Some(-32.9)
        );
        let pce = "The PCE price index for August increased 0.3 percent . Excluding food and energy, the PCE price index increased 0.2 percent.";
        assert_eq!(parse_pce(pce), Some((0.3, 0.2)));
        let down = "The PCE price index decreased 0.5 percent (table 9). Excluding food and energy, the PCE price index decreased 0.4 percent";
        assert_eq!(parse_pce(down), Some((-0.5, -0.4)));
        assert_eq!(
            month_label("Personal Income and Outlays, August 2026").as_deref(),
            Some("2026年8月分")
        );
    }
}
