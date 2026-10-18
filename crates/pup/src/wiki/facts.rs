//! Facts the wiki may cite, and the check that every number the LLM writes is one of them.
//!
//! Scopes: an event id (`fomc-2025-09-17`: the release's facts plus every market window) or an
//! event type (`type-fomc`: aggregates by sub-group). Footnotes cite `fact:{scope}/{key}` and must
//! repeat the fact's text. Every number with a unit in the prose must carry a footnote in the same
//! clause (before the next number or 「。」) and equal that footnote's fact (a page-wide match is too weak: with ~200 facts per event, a
//! wrong number often equals some other fact), and a direction word after it (上昇/下落/…) must
//! agree with the fact's sign.

use super::Source;
use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct FactRow {
    pub key: String,
    pub text: String,
}

pub struct Facts {
    src: Source,
    cache: RefCell<BTreeMap<String, Rc<Vec<FactRow>>>>,
    events: RefCell<Option<Rc<Vec<Value>>>>,
}

fn rows(v: &Value) -> Vec<FactRow> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some(FactRow {
                key: f["key"].as_str()?.into(),
                text: f["text"].as_str()?.into(),
            })
        })
        .collect()
}

impl Facts {
    pub fn new(src: Source) -> Self {
        Self {
            src,
            cache: RefCell::new(BTreeMap::new()),
            events: RefCell::new(None),
        }
    }

    pub fn source(&self) -> &Source {
        &self.src
    }

    /// The published event list (newest first).
    pub fn events(&self) -> Result<Rc<Vec<Value>>> {
        if let Some(e) = self.events.borrow().as_ref() {
            return Ok(e.clone());
        }
        let v = self
            .src
            .get("events.json")?
            .context("events.json not found")?;
        let e = Rc::new(v.as_array().cloned().unwrap_or_default());
        *self.events.borrow_mut() = Some(e.clone());
        Ok(e)
    }

    pub fn event_json(&self, id: &str) -> Result<Option<Value>> {
        self.src.get(&format!("events/{id}.json"))
    }

    pub fn type_json(&self, kind: &str) -> Result<Option<Value>> {
        self.src.get(&format!("types/{kind}.json"))
    }

    pub fn scope(&self, scope: &str) -> Result<Rc<Vec<FactRow>>> {
        if let Some(r) = self.cache.borrow().get(scope) {
            return Ok(r.clone());
        }
        let list = if let Some(kind) = scope.strip_prefix("type-") {
            self.type_json(kind)?
                .map(|v| rows(&v["facts"]))
                .unwrap_or_default()
        } else {
            match self.event_json(scope)? {
                Some(v) => {
                    let mut r = rows(&v["event"]["facts"]);
                    for m in v["markets"].as_array().into_iter().flatten() {
                        r.extend(rows(&m["facts"]));
                    }
                    r
                }
                None => Vec::new(),
            }
        };
        let rc = Rc::new(list);
        self.cache
            .borrow_mut()
            .insert(scope.to_string(), rc.clone());
        Ok(rc)
    }

    pub fn lookup(&self, scope: &str, key: &str) -> Result<Option<FactRow>> {
        Ok(self.scope(scope)?.iter().find(|f| f.key == key).cloned())
    }

    pub fn allowed(&self, scopes: &[String]) -> Result<Allowed> {
        let mut a = Allowed::default();
        for s in scopes {
            for f in self.scope(s)?.iter() {
                a.add(&f.text);
            }
        }
        Ok(a)
    }
}

/// A number with a unit as written in text.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub sign: i8,
    pub explicit_sign: bool,
    pub mag: String,
    pub unit: String,
    pub end: usize,
    pub raw: String,
}

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"([+\-−▲△]?)(\d+(?:\.\d+)?)(?:〜(\d+(?:\.\d+)?))?\s?(%|％|bp|pt|ポイント|千人|千件)",
        )
        .unwrap()
    })
}

fn norm_unit(u: &str) -> &str {
    match u {
        "％" => "%",
        "ポイント" => "pt",
        x => x,
    }
}

pub fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    for c in token_re().captures_iter(text) {
        let s = c.get(1).unwrap().as_str();
        let sign = match s {
            "+" => 1,
            "-" | "−" | "▲" | "△" => -1,
            _ => 0,
        };
        let unit = norm_unit(&c[4]).to_string();
        let m = c.get(0).unwrap();
        out.push(Token {
            sign,
            explicit_sign: sign != 0,
            mag: c[2].to_string(),
            unit: unit.clone(),
            end: m.end(),
            raw: m.as_str().to_string(),
        });
        if let Some(hi) = c.get(3) {
            out.push(Token {
                sign: 0,
                explicit_sign: false,
                mag: hi.as_str().into(),
                unit,
                end: m.end(),
                raw: m.as_str().to_string(),
            });
        }
    }
    out
}

#[derive(Debug, Default)]
pub struct Allowed {
    signed: HashSet<(i8, String, String)>,
    mag: HashSet<(String, String)>,
}

impl Allowed {
    pub fn add(&mut self, fact_text: &str) {
        for t in tokens(fact_text) {
            // "+0.00%" is neither up nor down.
            let sign = if t.mag.trim_start_matches(['0', '.']).is_empty() {
                0
            } else {
                t.sign
            };
            self.signed.insert((sign, t.mag.clone(), t.unit.clone()));
            self.mag.insert((t.mag, t.unit));
        }
    }
}

const UP: &[&str] = &[
    "上昇",
    "上げ",
    "上振れ",
    "高く",
    "高まっ",
    "拡大",
    "反発",
    "プラス",
];
const DOWN: &[&str] = &[
    "下落",
    "下げ",
    "下振れ",
    "低下",
    "安く",
    "縮小",
    "反落",
    "マイナス",
];

/// Direction stated right after a number, e.g. `0.52%上昇` / `0.52%の下落`.
pub fn implied_sign(after: &str) -> Option<i8> {
    let window: String = after.chars().take(6).collect();
    let stop = window
        .find(['、', '。', '（', '(', ',', '.'])
        .unwrap_or(window.len());
    let w = &window[..stop];
    let up = UP.iter().filter_map(|k| w.find(k)).min();
    let down = DOWN.iter().filter_map(|k| w.find(k)).min();
    match (up, down) {
        (Some(u), Some(d)) => Some(if u < d { 1 } else { -1 }),
        (Some(_), None) => Some(1),
        (None, Some(_)) => Some(-1),
        _ => None,
    }
}

/// Citations that passed the check on an earlier day: `scope/key` → the fact texts they matched.
///
/// Facts move after a page is written (aggregates change as events are added, BLS revises
/// seasonally adjusted figures, a "previous release" fact changes when a missing release is
/// filled in). A footnote verified on its day stays valid as a record of that day; only new
/// citations must equal today's facts. Stored in the wiki as `.pup/cited.json`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Ledger {
    pub cited: BTreeMap<String, BTreeSet<String>>,
}

impl Ledger {
    fn path(wiki: &std::path::Path) -> std::path::PathBuf {
        wiki.join(".pup").join("cited.json")
    }

    pub fn load(wiki: &std::path::Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(Self::path(wiki)).ok()?).ok()
    }

    pub fn save(&self, wiki: &std::path::Path) -> Result<()> {
        let p = Self::path(wiki);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn add(&mut self, scope: &str, key: &str, text: &str) {
        self.cited
            .entry(format!("{scope}/{key}"))
            .or_default()
            .insert(text.trim().to_string());
    }

    /// A recorded text for this citation that the footnote repeats.
    pub fn known(&self, scope: &str, key: &str, rest: &str) -> Option<String> {
        self.cited
            .get(&format!("{scope}/{key}"))?
            .iter()
            .find(|t| rest.contains(t.as_str()))
            .cloned()
    }
}

/// `(scope, key, text)` of every footnote in an LLM block.
pub fn footnotes(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|l| footnote_re().captures(l.trim()))
        .map(|c| (c[1].to_string(), c[2].to_string(), c[3].trim().to_string()))
        .collect()
}

pub fn footnote_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\[\^[^\]]+\]:\s*fact:([a-z0-9\-]+)/([A-Za-z0-9_.\-]+)\s+(.+)$").unwrap()
    })
}

fn fact_token(text: &str) -> Option<Token> {
    tokens(text).into_iter().next()
}

/// The footnote that belongs to a number: the first `[^label]` after it, before the next number
/// or the end of the sentence (`+1.15%[^7]`, `2.40pt 低下していた[^5]`). Returns the label and the
/// text between the number and the footnote (where a direction word may be).
fn footnote_after(after: &str) -> Option<(&str, &str)> {
    let i = after.find("[^")?;
    let between = &after[..i];
    if between.contains('。') || !tokens(between).is_empty() {
        return None;
    }
    let label = after[i + 2..].split_once(']')?.0;
    Some((label, between))
}

fn sign_ok(fact: &Token, written_sign: Option<i8>) -> bool {
    let zero = fact.mag.trim_start_matches(['0', '.']).is_empty();
    match written_sign {
        None => true,
        Some(_) if fact.sign == 0 || zero => true, // levels, or no move: direction words describe something else
        Some(s) => s == fact.sign,
    }
}

/// Problems in an LLM block given the facts it may use.
pub fn check_llm(
    text: &str,
    facts: &Facts,
    scopes: &[String],
    _allowed: &Allowed,
    ledger: &Ledger,
) -> Result<Vec<String>> {
    let mut errs = Vec::new();
    // Footnote label → the cited fact's text.
    let mut notes: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in text.lines() {
        let l = line.trim();
        if !(l.starts_with("[^") && l.contains("]:")) {
            continue;
        }
        let Some(c) = footnote_re().captures(l) else {
            errs.push(format!(
                "footnote is not `[^n]: fact:{{scope}}/{{key}} {{text}}`: {l}"
            ));
            continue;
        };
        let label = l[2..l.find(']').unwrap_or(2)].to_string();
        let (scope, key, rest) = (&c[1], &c[2], &c[3]);
        if !scopes.iter().any(|s| s == scope) {
            errs.push(format!(
                "footnote cites `{scope}`, which this page may not use: {l}"
            ));
            continue;
        }
        let current = facts.lookup(scope, key)?;
        let text = match &current {
            Some(f) if rest.contains(&f.text) => Some(f.text.clone()),
            // Verified on an earlier day; the fact has moved since.
            _ => ledger.known(scope, key, rest),
        };
        match (text, current) {
            (Some(t), _) => {
                if notes.insert(label.clone(), t).is_some() {
                    errs.push(format!("footnote `[^{label}]` is defined twice"));
                }
            }
            (None, None) => errs.push(format!("no fact `{scope}/{key}`: {l}")),
            (None, Some(f)) => errs.push(format!(
                "footnote text must contain `{}` for `{scope}/{key}`: {l}",
                f.text
            )),
        }
    }
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('#') || (l.starts_with("[^") && l.contains("]:")) {
            continue; // headings carry dates and titles only; footnotes were checked above
        }
        for t in tokens(l) {
            let after = &l[t.end..];
            let Some((label, between)) = footnote_after(after) else {
                errs.push(format!(
                    "`{}` has no footnote in the same clause: {l}",
                    t.raw
                ));
                continue;
            };
            let Some(text) = notes.get(label) else {
                errs.push(format!(
                    "`{}` cites `[^{label}]`, which is not a valid footnote: {l}",
                    t.raw
                ));
                continue;
            };
            // A range fact (`4.00〜4.25%`) may be cited by either end.
            let candidates: Vec<Token> = tokens(text);
            let after_note = after[between.len()..]
                .split_once(']')
                .map(|(_, r)| r)
                .unwrap_or("");
            let written = if t.explicit_sign {
                Some(t.sign)
            } else {
                implied_sign(between).or_else(|| implied_sign(after_note))
            };
            let ok = candidates.iter().any(|f| {
                f.mag == t.mag
                    && f.unit == t.unit
                    && (!t.explicit_sign || f.sign == t.sign || f.sign == 0)
                    && sign_ok(f, written)
            });
            if !ok {
                let f = fact_token(text)
                    .map(|f| f.raw)
                    .unwrap_or_else(|| text.clone());
                errs.push(format!(
                    "`{}` does not match its footnote `[^{label}]` ({f}): {l}",
                    t.raw
                ));
            }
        }
    }
    Ok(errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(texts: &[&str]) -> Allowed {
        let mut a = Allowed::default();
        for t in texts {
            a.add(t);
        }
        a
    }

    #[test]
    fn extracts_tokens_and_ranges() {
        let t = tokens("S&P 500 は -0.10%、金利は 4.00〜4.25% で +25bp、雇用は +254千人");
        let raw: Vec<(i8, &str, &str)> = t
            .iter()
            .map(|t| (t.sign, t.mag.as_str(), t.unit.as_str()))
            .collect();
        assert_eq!(
            raw,
            vec![
                (-1, "0.10", "%"),
                (0, "4.00", "%"),
                (0, "4.25", "%"),
                (1, "25", "bp"),
                (1, "254", "千人")
            ]
        );
    }

    #[test]
    fn ledger_accepts_only_recorded_texts() {
        let mut l = Ledger::default();
        l.add("type-boj", "boj.all.spx.r20.mean", "+1.85%");
        // Verified when written; today's fact is +1.84%.
        assert_eq!(
            l.known("type-boj", "boj.all.spx.r20.mean", "+1.85%")
                .as_deref(),
            Some("+1.85%")
        );
        // A number never verified is not accepted, nor is another key.
        assert_eq!(l.known("type-boj", "boj.all.spx.r20.mean", "+1.90%"), None);
        assert_eq!(l.known("type-boj", "boj.all.spx.r5.mean", "+1.85%"), None);
        let notes = footnotes("text\n[^3]: fact:type-boj/boj.all.spx.r20.mean +1.85%\n");
        assert_eq!(
            notes,
            vec![(
                "type-boj".into(),
                "boj.all.spx.r20.mean".into(),
                "+1.85%".into()
            )]
        );
    }

    #[test]
    fn footnote_after_number() {
        assert_eq!(footnote_after("[^12]、日経").map(|x| x.0), Some("12"));
        assert_eq!(
            footnote_after(" 低下していた[^5]。").map(|x| x.0),
            Some("5")
        );
        assert_eq!(footnote_after("、日経平均は +1.38%[^13]"), None); // belongs to the next number
        assert_eq!(footnote_after("。次に[^4]"), None);
    }

    #[test]
    fn direction_words() {
        assert_eq!(implied_sign("上昇した"), Some(1));
        assert_eq!(implied_sign("の下落となった"), Some(-1));
        assert_eq!(implied_sign("、その後上昇"), None);
    }

    #[test]
    fn sign_rules() {
        let down = fact_token("-0.10%").unwrap();
        assert!(sign_ok(&down, Some(-1)));
        assert!(!sign_ok(&down, Some(1)));
        let level = fact_token("4.25%").unwrap();
        assert!(sign_ok(&level, Some(1))); // 「4.25% に上昇」 describes a level
        let a = allowed(&["4.00〜4.25%"]);
        assert!(a.mag.contains(&("4.00".into(), "%".into())));
    }
}
