//! What we track: market indices (with their closing times) and economic event types.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;

/// How a move is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// Percentage change of the price (equity indices, FX, commodities).
    Percent,
    /// Change in index points (VIX).
    Points,
    /// Change in basis points of a yield quoted in percent (US 10Y).
    BasisPoints,
}

impl Measure {
    pub fn unit(self) -> &'static str {
        match self {
            Measure::Percent => "%",
            Measure::Points => "pt",
            Measure::BasisPoints => "bp",
        }
    }

    /// Decimals used everywhere a value of this measure is written (facts, wiki, checks).
    pub fn decimals(self) -> usize {
        match self {
            Measure::Percent | Measure::Points => 2,
            Measure::BasisPoints => 1,
        }
    }

    /// Move from `base` to `value` in this measure.
    pub fn change(self, base: f64, value: f64) -> f64 {
        match self {
            Measure::Percent => (value / base - 1.0) * 100.0,
            Measure::Points => value - base,
            Measure::BasisPoints => (value - base) * 100.0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MarketDef {
    pub id: &'static str,
    pub symbol: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub measure: Measure,
    /// IANA zone and local time of the daily close used to decide whether a bar is before or after an event.
    pub tz: &'static str,
    pub close: (u32, u32),
    /// Pages for these markets get trend write-ups in the wiki; the rest appear in tables and charts only.
    pub wiki: bool,
}

impl MarketDef {
    pub fn tz(&self) -> Tz {
        self.tz.parse().expect("valid tz in MARKETS")
    }

    /// UTC instant of the close for a bar dated `date` in the market's local calendar.
    pub fn close_utc(&self, date: NaiveDate) -> DateTime<Utc> {
        let (h, m) = self.close;
        let local = date.and_hms_opt(h, m, 0).expect("valid close time");
        self.tz()
            .from_local_datetime(&local)
            .earliest()
            .expect("close time exists")
            .with_timezone(&Utc)
    }
}

#[allow(clippy::too_many_arguments)] // a table row; named fields would make MARKETS unreadable
const fn m(
    id: &'static str,
    symbol: &'static str,
    name: &'static str,
    group: &'static str,
    measure: Measure,
    tz: &'static str,
    close: (u32, u32),
    wiki: bool,
) -> MarketDef {
    MarketDef {
        id,
        symbol,
        name,
        group,
        measure,
        tz,
        close,
        wiki,
    }
}

/// Yahoo Finance symbols. Closing times are the ones that bound the daily bar; for 24h markets
/// (FX, crypto) the bar is cut at the end of the UTC day.
pub const MARKETS: &[MarketDef] = &[
    m(
        "spx",
        "^GSPC",
        "S&P 500",
        "米国株",
        Measure::Percent,
        "America/New_York",
        (16, 0),
        true,
    ),
    m(
        "ndx",
        "^NDX",
        "Nasdaq 100",
        "米国株",
        Measure::Percent,
        "America/New_York",
        (16, 0),
        false,
    ),
    m(
        "rut",
        "^RUT",
        "Russell 2000",
        "米国株",
        Measure::Percent,
        "America/New_York",
        (16, 0),
        false,
    ),
    m(
        "vix",
        "^VIX",
        "VIX",
        "米国株",
        Measure::Points,
        "America/New_York",
        (16, 15),
        true,
    ),
    m(
        "n225",
        "^N225",
        "日経平均",
        "日本株",
        Measure::Percent,
        "Asia/Tokyo",
        (15, 30),
        true,
    ),
    m(
        "topix",
        "1306.T",
        "TOPIX（1306 ETF）",
        "日本株",
        Measure::Percent,
        "Asia/Tokyo",
        (15, 30),
        false,
    ),
    m(
        "dax",
        "^GDAXI",
        "DAX",
        "欧州株",
        Measure::Percent,
        "Europe/Berlin",
        (17, 30),
        false,
    ),
    m(
        "sx5e",
        "^STOXX50E",
        "Euro Stoxx 50",
        "欧州株",
        Measure::Percent,
        "Europe/Berlin",
        (17, 30),
        false,
    ),
    m(
        "ftse",
        "^FTSE",
        "FTSE 100",
        "欧州株",
        Measure::Percent,
        "Europe/London",
        (16, 30),
        false,
    ),
    m(
        "hsi",
        "^HSI",
        "ハンセン",
        "アジア株",
        Measure::Percent,
        "Asia/Hong_Kong",
        (16, 0),
        false,
    ),
    m(
        "us10y",
        "^TNX",
        "米10年金利",
        "金利・為替",
        Measure::BasisPoints,
        "America/New_York",
        (15, 0),
        true,
    ),
    m(
        "usdjpy",
        "JPY=X",
        "ドル円",
        "金利・為替",
        Measure::Percent,
        "UTC",
        (23, 59),
        true,
    ),
    m(
        "eurusd",
        "EURUSD=X",
        "ユーロドル",
        "金利・為替",
        Measure::Percent,
        "UTC",
        (23, 59),
        false,
    ),
    m(
        "gbpusd",
        "GBPUSD=X",
        "ポンドドル",
        "金利・為替",
        Measure::Percent,
        "UTC",
        (23, 59),
        false,
    ),
    m(
        "dxy",
        "DX-Y.NYB",
        "ドル指数",
        "金利・為替",
        Measure::Percent,
        "America/New_York",
        (17, 0),
        false,
    ),
    m(
        "gold",
        "GC=F",
        "金先物",
        "商品・暗号資産",
        Measure::Percent,
        "America/New_York",
        (17, 0),
        false,
    ),
    m(
        "wti",
        "CL=F",
        "WTI 原油先物",
        "商品・暗号資産",
        Measure::Percent,
        "America/New_York",
        (17, 0),
        false,
    ),
    m(
        "btc",
        "BTC-USD",
        "ビットコイン",
        "商品・暗号資産",
        Measure::Percent,
        "UTC",
        (23, 59),
        false,
    ),
];

pub fn market(id: &str) -> Option<&'static MarketDef> {
    MARKETS.iter().find(|m| m.id == id)
}

#[derive(Debug, Clone, Serialize)]
pub struct EventTypeDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub const EVENT_TYPES: &[EventTypeDef] = &[
    EventTypeDef {
        id: "fomc",
        name: "FOMC",
        description: "米連邦公開市場委員会の政策金利の決定（声明公表）",
    },
    EventTypeDef {
        id: "boj",
        name: "日銀金融政策決定会合",
        description: "日本銀行の金融政策決定会合（最終日）",
    },
    EventTypeDef {
        id: "ecb",
        name: "ECB 理事会",
        description: "欧州中央銀行の金融政策の決定（理事会）",
    },
    EventTypeDef {
        id: "boe",
        name: "BoE 金融政策委員会",
        description: "イングランド銀行の政策金利（Bank Rate）の決定（MPC）",
    },
    EventTypeDef {
        id: "jpcpi",
        name: "日本CPI",
        description: "全国消費者物価指数の公表（総務省統計局、8:30）",
    },
    EventTypeDef {
        id: "tankan",
        name: "日銀短観",
        description: "全国企業短期経済観測調査の公表（日本銀行、8:50）",
    },
    EventTypeDef {
        id: "cpi",
        name: "米CPI",
        description: "米消費者物価指数の公表（BLS）",
    },
    EventTypeDef {
        id: "nfp",
        name: "米雇用統計",
        description: "米雇用統計（非農業部門雇用者数・失業率）の公表（BLS）",
    },
    EventTypeDef {
        id: "ppi",
        name: "米PPI",
        description: "米生産者物価指数（最終需要）の公表（BLS）",
    },
    EventTypeDef {
        id: "jolts",
        name: "米JOLTS",
        description: "米求人・離職動態調査（求人件数）の公表（BLS、10:00）",
    },
];

pub fn event_type(id: &str) -> Option<&'static EventTypeDef> {
    EVENT_TYPES.iter().find(|t| t.id == id)
}

/// Trading-day offsets kept around each event.
pub const PRE_SESSIONS: i64 = 20;
pub const POST_SESSIONS: i64 = 20;
/// Horizons (sessions after the event) reported as facts.
pub const HORIZONS: &[usize] = &[1, 2, 5, 10, 20];
