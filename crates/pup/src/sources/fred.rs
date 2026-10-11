//! Release dates from the FRED API (free key in `FRED_API_KEY`).

use crate::http;
use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde_json::Value;

pub const RELEASE_CPI: u32 = 10;
pub const RELEASE_EMPLOYMENT: u32 = 50;
pub const RELEASE_PPI: u32 = 46;
pub const RELEASE_JOLTS: u32 = 192;

pub fn release_dates(
    client: &reqwest::blocking::Client,
    key: &str,
    release_id: u32,
    from: NaiveDate,
) -> Result<Vec<NaiveDate>> {
    let url = format!(
        "https://api.stlouisfed.org/fred/release/dates?release_id={release_id}&api_key={key}&file_type=json&realtime_start={from}&include_release_dates_with_no_data=false&limit=10000&sort_order=asc"
    );
    let body: Value = serde_json::from_str(
        &http::get_text(client, &url)
            .map_err(|e| anyhow::anyhow!("FRED release {release_id}: {}", e.root_cause()))?,
    )?;
    body["release_dates"]
        .as_array()
        .context("no release_dates")?
        .iter()
        .filter_map(|d| d["date"].as_str())
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").context("date"))
        .collect()
}
