//! Daily closes from Yahoo Finance's chart endpoint.

use crate::config::MarketDef;
use crate::http;
use crate::model::{Bar, Series};
use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use serde_json::Value;

pub fn fetch(
    client: &reqwest::blocking::Client,
    market: &MarketDef,
    from: chrono::NaiveDate,
) -> Result<Series> {
    let p1 = Utc
        .from_utc_datetime(&from.and_hms_opt(0, 0, 0).unwrap())
        .timestamp();
    let p2 = Utc::now().timestamp();
    let sym = market.symbol.replace('^', "%5E").replace('=', "%3D");
    let url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{sym}?period1={p1}&period2={p2}&interval=1d&events=div%2Csplits");
    let body: Value = serde_json::from_str(&http::get_text(client, &url)?)
        .with_context(|| format!("parse {url}"))?;
    parse(market, &body)
}

pub fn parse(market: &MarketDef, body: &Value) -> Result<Series> {
    let r = &body["chart"]["result"][0];
    let tz: chrono_tz::Tz = r["meta"]["exchangeTimezoneName"]
        .as_str()
        .unwrap_or(market.tz)
        .parse()
        .unwrap_or(market.tz());
    let ts = r["timestamp"].as_array().context("no timestamps")?;
    // Adjusted close removes dividend drops (matters for the TOPIX ETF); indices have none.
    let adj = r["indicators"]["adjclose"][0]["adjclose"].as_array();
    let close = r["indicators"]["quote"][0]["close"]
        .as_array()
        .context("no closes")?;
    let mut bars: Vec<Bar> = Vec::with_capacity(ts.len());
    for (i, t) in ts.iter().enumerate() {
        let v = adj
            .and_then(|a| a.get(i))
            .and_then(Value::as_f64)
            .or_else(|| close.get(i).and_then(Value::as_f64));
        let (Some(t), Some(v)) = (t.as_i64(), v) else {
            continue;
        };
        if !v.is_finite() || v <= 0.0 && market.measure == crate::config::Measure::Percent {
            continue;
        }
        let date = tz
            .timestamp_opt(t, 0)
            .single()
            .context("bad timestamp")?
            .date_naive();
        match bars.last_mut() {
            Some(last) if last.date == date => last.close = v, // the live bar can repeat the last date
            _ => bars.push(Bar { date, close: v }),
        }
    }
    Ok(Series {
        id: market.id.to_string(),
        symbol: market.symbol.to_string(),
        bars,
    })
}
