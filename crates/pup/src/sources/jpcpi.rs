//! Japan's national CPI (Statistics Bureau) as first published: e-Stat lists one page per
//! reference month for each base year; each page links the release summary PDF (結果の概要（全国）),
//! whose first page carries the release date and the headline year-on-year rates.

use crate::http;
use crate::model::{Event, Fact, SourceRef};
use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveTime, TimeZone};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const ESTAT: &str = "https://www.e-stat.go.jp";
const FIRST_YEAR: i32 = 2016;

/// `(base year, tstat, tclass1)` of the monthly result lists on e-Stat (toukei 00200573).
const BASES: &[(&str, &str, &str)] = &[
    ("2010", "000001044944", "000001044979"),
    ("2015", "000001084976", "000001085955"),
    ("2020", "000001150147", "000001150149"),
    ("2025", "000001243876", "000001243877"),
];

/// One release, cached per reference month once its summary has been read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub month: String,
    pub base: String,
    pub released: NaiveDate,
    pub headline: Option<f64>,
    pub core: Option<f64>,
    pub core_core: Option<f64>,
    pub url: String,
}

/// Reference-month pages in a base year's list: `(YYYY-MM, url)`.
fn month_pages(list: &str) -> Vec<(String, String)> {
    let re = Regex::new(
        r#"href="(/stat-search/files\?[^"]*?year=(\d{4})0&amp;month=(\d+)[^"]*)"[^>]*>\s*(\d+)月\s*<"#,
    )
    .unwrap();
    re.captures_iter(list)
        .filter_map(|c| {
            let y: i32 = c[2].parse().ok()?;
            let m: u32 = c[4].parse().ok()?;
            (y >= FIRST_YEAR).then(|| {
                (
                    format!("{y}-{m:02}"),
                    format!("{ESTAT}{}", c[1].replace("&amp;", "&")),
                )
            })
        })
        .collect()
}

/// The summary PDF's id on a month page: the `fileKind=2` link after 「結果の概要（全国）」.
fn summary_id(page: &str) -> Option<String> {
    let at = page.find("結果の概要（全国）")?;
    let seg: String = page[at..].chars().take(4000).collect();
    let re = Regex::new(r"statInfId=(\d+)&(?:amp;)?fileKind=2").unwrap();
    Some(re.captures(&seg)?[1].to_string())
}

/// Release date and y/y rates from the summary's first page. Era dates: 平成 = 1988 + n,
/// 令和 = 2018 + n (元 = 1).
/// What the summary's first page says: the release date and three y/y rates.
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub released: NaiveDate,
    pub headline: Option<f64>,
    pub core: Option<f64>,
    pub core_core: Option<f64>,
}

pub fn parse_summary(text: &str) -> Option<Summary> {
    let flat: String = http::ascii_digits(text)
        .chars()
        .filter(|c| !matches!(c, ' ' | '　' | '\n' | '\r' | '\t'))
        .collect();
    let d = Regex::new(r"(平成|令和)(元|\d+)年(\d+)月(\d+)日").unwrap();
    let c = d.captures(&flat)?;
    let n: i32 = if &c[2] == "元" {
        1
    } else {
        c[2].parse().ok()?
    };
    let year = if &c[1] == "平成" {
        1988 + n
    } else {
        2018 + n
    };
    let released = NaiveDate::from_ymd_opt(year, c[3].parse().ok()?, c[4].parse().ok()?)?;
    let yoy = |label: &str| -> Option<f64> {
        let at = flat.find(label)?;
        let seg: String = flat[at..].chars().take(120).collect();
        let re = Regex::new(
            r"前年同月比は(-?[\d.]+)[%％]の(上昇|下落)|前年同月比は(同水準|変わらず)|前年同月と(同水準)",
        )
        .unwrap();
        let m = re.captures(&seg)?;
        match m.get(1) {
            Some(v) => {
                let v: f64 = v.as_str().parse().ok()?;
                Some(if &m[2] == "下落" { -v } else { v })
            }
            None => Some(0.0),
        }
    };
    // The core labels contain 「総合指数は」 too, so the headline is its first occurrence.
    Some(Summary {
        released,
        headline: yoy("総合指数は"),
        core: yoy("生鮮食品を除く総合指数は"),
        core_core: yoy("生鮮食品及びエネルギーを除く総合指数は"),
    })
}

pub fn releases(client: &reqwest::blocking::Client, cache: &Path) -> Result<Vec<Release>> {
    std::fs::create_dir_all(cache)?;
    let mut out: BTreeMap<String, Release> = BTreeMap::new();
    for (base, tstat, tclass) in BASES {
        let url = format!(
            "{ESTAT}/stat-search/files?page=1&layout=datalist&toukei=00200573&tstat={tstat}&cycle=1&tclass1={tclass}"
        );
        let list = match http::get_text(client, &url) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("jpcpi list {base}: {e:#}");
                continue;
            }
        };
        for (month, page_url) in month_pages(&list) {
            if out.contains_key(&month) {
                continue;
            }
            let cached = cache.join(format!("{month}.json"));
            if let Some(r) = std::fs::read(&cached)
                .ok()
                .and_then(|b| serde_json::from_slice::<Release>(&b).ok())
            {
                out.insert(month, r);
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
            let r = (|| -> Result<Option<Release>> {
                let page = http::get_text(client, &page_url)?;
                let Some(id) = summary_id(&page) else {
                    return Ok(None);
                };
                let pdf_url =
                    format!("{ESTAT}/stat-search/file-download?statInfId={id}&fileKind=2");
                let text = http::pdf_first_page(&http::get_bytes(client, &pdf_url)?, false)?;
                let s = parse_summary(&text).context("no release date in the summary")?;
                Ok(Some(Release {
                    month: month.clone(),
                    base: base.to_string(),
                    released: s.released,
                    headline: s.headline,
                    core: s.core,
                    core_core: s.core_core,
                    url: pdf_url,
                }))
            })();
            match r {
                Ok(Some(r)) => {
                    std::fs::write(&cached, serde_json::to_vec(&r)?)?;
                    out.insert(month, r);
                }
                Ok(None) => tracing::warn!("jpcpi {month}: no summary yet"),
                Err(e) => tracing::warn!("jpcpi {month}: {e:#}"),
            }
        }
    }
    Ok(out.into_values().collect())
}

pub fn events(releases: &[Release]) -> Result<Vec<Event>> {
    let tokyo: chrono_tz::Tz = "Asia/Tokyo".parse().unwrap();
    let mut events = Vec::new();
    let mut prev: Option<&Release> = None;
    for r in releases {
        let (y, m) = r.month.split_once('-').context("month")?;
        let label = format!("{y}年{}月分", m.trim_start_matches('0'));
        let mut facts = vec![
            Fact::text("ref_month", "対象月", label.clone()),
            Fact::text("base", "基準年", format!("{}年基準", r.base)),
        ];
        if let Some(v) = r.headline {
            facts.push(Fact::num("cpi_yoy", "総合（前年同月比）", v, 1, "%"));
        }
        if let Some(v) = r.core {
            facts.push(Fact::num(
                "core_yoy",
                "生鮮食品を除く総合（前年同月比）",
                v,
                1,
                "%",
            ));
        }
        if let Some(v) = r.core_core {
            facts.push(Fact::num(
                "core_core_yoy",
                "生鮮食品及びエネルギーを除く総合（前年同月比）",
                v,
                1,
                "%",
            ));
        }
        if let (Some(a), Some(b)) = (r.core, prev.and_then(|p| p.core)) {
            facts.push(Fact::num(
                "core_yoy_prev",
                "前月の生鮮食品を除く総合",
                b,
                1,
                "%",
            ));
            facts.push(Fact::signed(
                "core_yoy_change",
                "生鮮食品を除く総合の前月からの変化",
                ((a - b) * 10.0).round() / 10.0,
                1,
                "pt",
            ));
        }
        let time_utc = tokyo
            .from_local_datetime(
                &r.released
                    .and_time(NaiveTime::from_hms_opt(8, 30, 0).unwrap()),
            )
            .earliest()
            .context("release time")?
            .with_timezone(&chrono::Utc);
        events.push(Event {
            id: format!("jpcpi-{}", r.released),
            kind: "jpcpi".into(),
            title: format!("日本CPI {label}"),
            date: r.released,
            time_utc,
            time_approx: false,
            facts,
            sources: vec![SourceRef {
                label: "総務省統計局 消費者物価指数 結果の概要（e-Stat）".into(),
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

    #[test]
    fn parses_summary_first_page() {
        let text = "令和８年９月18日\n総 務 省\n消費者物価指数（ＣＰＩ）\n（１）総合指数は２０２０年を１００として１１２．０\n前年同月比は１．９％の上昇\n（２）生鮮食品を除く総合指数は１１１．５\n前年同月比は１．７％の上昇\n（３）生鮮食品及びエネルギーを除く総合指数は１１０．１\n前年同月比は１．９％の上昇";
        let s = parse_summary(text).unwrap();
        assert_eq!(s.released, NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        assert_eq!(
            (s.headline, s.core, s.core_core),
            (Some(1.9), Some(1.7), Some(1.9))
        );
        let older = "平成30年２月23日\n（１）総合指数は前年同月比は1.4％の上昇\n（２）生鮮食品を除く総合指数は前年同月比は0.4％の下落";
        let s = parse_summary(older).unwrap();
        assert_eq!(s.released, NaiveDate::from_ymd_opt(2018, 2, 23).unwrap());
        assert_eq!(
            (s.headline, s.core, s.core_core),
            (Some(1.4), Some(-0.4), None)
        );
    }

    #[test]
    fn finds_month_pages_and_summary() {
        let list = r#"<a href="/stat-search/files?page=1&amp;layout=datalist&amp;toukei=00200573&amp;year=20260&amp;month=24101212&amp;tclass1=000001243877">8月</a>
<a href="/stat-search/files?page=1&amp;year=20150&amp;month=24101212">8月</a>"#;
        let pages = month_pages(list);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].0, "2026-08");
        assert!(pages[0].1.contains("&year=20260&month=24101212"));
        let page = "<td>結果の概要（全国）</td><td>2026-09-18</td><a href=\"/stat-search/file-download?statInfId=000040312345&fileKind=2\">PDF</a>";
        assert_eq!(summary_id(page).as_deref(), Some("000040312345"));
    }
}
