//! BLS release dates without a FRED key. bls.gov refuses scripted clients, but every release is
//! archived as `news.release/archives/{name}_{MMDDYYYY}.htm`, and the Internet Archive's CDX index
//! of those URLs gives the dates. A release shows up once the Archive has crawled it, which can lag
//! the release by a few days.

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
