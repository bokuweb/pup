//! BOJ Tankan (短観): release dates from the BOJ's summary index pages (five-year blocks), and
//! the large-enterprise business conditions DIs (業況判断DI, 最近) from each summary PDF.
//! The DI is a percentage-point balance, written with `pt` so the wiki checker covers it.

use crate::http;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, NaiveTime, TimeZone};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const BASE: &str = "https://www.boj.or.jp";
const FIRST_YEAR: i32 = 2016;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Survey {
    /// `YYMM` of the survey month (03/06/09/12).
    pub code: String,
    pub released: NaiveDate,
    pub large_mfg: i64,
    pub large_nonmfg: i64,
    pub url: String,
}

/// `(YYMM, release date, PDF path)` from an index page's table rows.
pub fn index_rows(html: &str) -> Vec<(String, NaiveDate, String)> {
    let row = Regex::new(r"(?s)<tr.*?</tr>").unwrap();
    let date = Regex::new(r"(\d{4})年\s*(\d{1,2})月\s*(\d{1,2})日").unwrap();
    let pdf = Regex::new(r#"href="(/statistics/tk/gaiyo/\d{4}/tka(\d{4})\.pdf)""#).unwrap();
    row.find_iter(html)
        .filter_map(|r| {
            let r = r.as_str();
            let text = http::html_text(r);
            let d = date.captures(&text)?;
            let p = pdf.captures(r)?;
            let released = NaiveDate::from_ymd_opt(
                d[1].parse().ok()?,
                d[2].parse().ok()?,
                d[3].parse().ok()?,
            )?;
            Some((p[2].to_string(), released, p[1].to_string()))
        })
        .collect()
}

/// Large manufacturers and large non-manufacturers, 最近: the third number on the first table line
/// starting with 製造業 / 非製造業 (columns: 大企業 前回最近, 先行き, 今回最近, …).
pub fn parse_dis(text: &str) -> Option<(i64, i64)> {
    let text = http::ascii_digits(text);
    let first = |label: &str| -> Option<i64> {
        text.lines().find_map(|line| {
            let rest = line.trim_start().strip_prefix(label)?;
            if !rest.starts_with(char::is_whitespace) {
                return None;
            }
            // Whole integer tokens only: `2019年度`, `2.` or `0.5%` are not DI cells.
            let nums: Vec<i64> = rest
                .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
                .filter_map(|t| t.parse().ok())
                .collect();
            (nums.len() >= 6).then(|| nums[2])
        })
    };
    let (m, n) = (first("製造業")?, first("非製造業")?);
    ((-100..=100).contains(&m) && (-100..=100).contains(&n)).then_some((m, n))
}

pub fn surveys(client: &reqwest::blocking::Client, cache: &Path) -> Result<Vec<Survey>> {
    std::fs::create_dir_all(cache)?;
    let mut rows = BTreeMap::new();
    let mut block = FIRST_YEAR;
    while block <= chrono::Utc::now().year() {
        let url = format!("{BASE}/statistics/tk/gaiyo/{block}/index.htm");
        match http::get_text(client, &url) {
            Ok(html) => {
                for (code, released, path) in index_rows(&html) {
                    rows.insert(code, (released, path));
                }
            }
            Err(e) => tracing::warn!("tankan {url}: {e:#}"),
        }
        block += 5;
    }
    let mut out = Vec::new();
    for (code, (released, path)) in rows {
        if released.year() < FIRST_YEAR {
            continue;
        }
        let cached = cache.join(format!("{code}.json"));
        if let Some(s) = std::fs::read(&cached)
            .ok()
            .and_then(|b| serde_json::from_slice::<Survey>(&b).ok())
        {
            out.push(s);
            continue;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        let url = format!("{BASE}{path}");
        let r = (|| -> Result<Survey> {
            let text = http::pdf_first_page(&http::get_bytes(client, &url)?, true)?;
            let (large_mfg, large_nonmfg) = parse_dis(&text).context("no DI table")?;
            Ok(Survey {
                code: code.clone(),
                released,
                large_mfg,
                large_nonmfg,
                url: url.clone(),
            })
        })();
        match r {
            Ok(s) => {
                std::fs::write(&cached, serde_json::to_vec(&s)?)?;
                out.push(s);
            }
            Err(e) => tracing::warn!("tankan {code}: {e:#}"),
        }
    }
    Ok(out)
}

pub fn events(surveys: &[Survey]) -> Result<Vec<Event>> {
    let tokyo: chrono_tz::Tz = "Asia/Tokyo".parse().unwrap();
    let mut events = Vec::new();
    let mut prev: Option<&Survey> = None;
    for s in surveys {
        let label = format!(
            "20{}年{}月調査",
            &s.code[..2],
            s.code[2..].trim_start_matches('0')
        );
        let mut facts = vec![
            Fact::text("survey", "調査", label.clone()),
            Fact::num(
                "large_mfg",
                "大企業製造業の業況判断DI（最近）",
                s.large_mfg as f64,
                0,
                "pt",
            ),
            Fact::num(
                "large_nonmfg",
                "大企業非製造業の業況判断DI（最近）",
                s.large_nonmfg as f64,
                0,
                "pt",
            ),
        ];
        if let Some(p) = prev {
            facts.push(Fact::num(
                "large_mfg_prev",
                "前回の大企業製造業DI",
                p.large_mfg as f64,
                0,
                "pt",
            ));
            facts.push(Fact::signed(
                "large_mfg_change",
                "大企業製造業DIの前回からの変化",
                (s.large_mfg - p.large_mfg) as f64,
                0,
                "pt",
            ));
            facts.push(Fact::signed(
                "large_nonmfg_change",
                "大企業非製造業DIの前回からの変化",
                (s.large_nonmfg - p.large_nonmfg) as f64,
                0,
                "pt",
            ));
        }
        let time_utc = tokyo
            .from_local_datetime(
                &s.released
                    .and_time(NaiveTime::from_hms_opt(8, 50, 0).unwrap()),
            )
            .earliest()
            .context("release time")?
            .with_timezone(&chrono::Utc);
        events.push(Event {
            id: format!("tankan-{}", s.released),
            kind: "tankan".into(),
            title: format!("日銀短観 {label}"),
            date: s.released,
            time_utc,
            time_approx: false,
            facts,
            sources: vec![SourceRef {
                label: "日本銀行 短観（概要）".into(),
                url: s.url.clone(),
            }],
        });
        prev = Some(s);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_index_rows() {
        let html = r#"<tr><td>2026年10月1日</td><td>2026年9月調査</td><td><a href="/statistics/tk/gaiyo/2026/tka2609.pdf">PDF</a></td></tr>
<tr><td>2026年7月1日</td><td><a href="/statistics/tk/gaiyo/2026/tka2606.zip">zip</a></td></tr>"#;
        let rows = index_rows(html);
        assert_eq!(
            rows,
            vec![(
                "2609".to_string(),
                NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
                "/statistics/tk/gaiyo/2026/tka2609.pdf".to_string()
            )]
        );
    }

    #[test]
    fn reads_large_enterprise_dis() {
        let text = "           大  企  業\n  製造業        -8      -27     -34      -26     -11    -19\n  非製造業       8      -1     -17      -14      -3     -9\n";
        assert_eq!(parse_dis(text), Some((-34, -17)));
        let fullwidth =
            "製造業　 ２２　 ２１　 ２４　 ２　 ２０　 ▲４\n非製造業 ３７ ３５ ３５ ▲２ ３４ ▲１\n";
        assert_eq!(parse_dis(fullwidth), Some((24, 35)));
    }
}
