//! Data shared by `collect` (raw inputs) and `build` (published facts).

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub date: NaiveDate,
    pub close: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub id: String,
    pub symbol: String,
    pub bars: Vec<Bar>,
}

/// One scheduled release. Facts are pre-formatted so the wiki and its checker use the same text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// `{type}-{YYYY-MM-DD}` (local release date).
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub date: NaiveDate,
    pub time_utc: DateTime<Utc>,
    /// True when the release time is a convention rather than published (BOJ).
    pub time_approx: bool,
    pub facts: Vec<Fact>,
    pub sources: Vec<SourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub key: String,
    pub label: String,
    /// Display text, e.g. `-25bp`, `4.00〜4.25%`, `利下げ`.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

impl Fact {
    pub fn text(key: &str, label: &str, text: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            text: text.into(),
            value: None,
        }
    }

    pub fn num(key: &str, label: &str, value: f64, decimals: usize, unit: &str) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            text: fmt_signed(value, decimals, unit, false),
            value: Some(value),
        }
    }

    pub fn signed(key: &str, label: &str, value: f64, decimals: usize, unit: &str) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            text: fmt_signed(value, decimals, unit, true),
            value: Some(value),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub label: String,
    pub url: String,
}

/// `+1.23%` / `-0.50bp` / `4.25%`. Minus is ASCII so facts and checks agree.
pub fn fmt_signed(v: f64, decimals: usize, unit: &str, sign: bool) -> String {
    let s = format!("{:.*}", decimals, v);
    let s = if s.starts_with('-') && s.trim_start_matches(['-', '0', '.']).is_empty() {
        s[1..].to_string()
    } else {
        s
    };
    if sign && !s.starts_with('-') {
        format!("+{s}{unit}")
    } else {
        format!("{s}{unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_signs_and_negative_zero() {
        assert_eq!(fmt_signed(1.234, 2, "%", true), "+1.23%");
        assert_eq!(fmt_signed(-0.004, 2, "%", true), "+0.00%");
        assert_eq!(fmt_signed(-12.36, 1, "bp", true), "-12.4bp");
        assert_eq!(fmt_signed(4.25, 2, "%", false), "4.25%");
    }
}
