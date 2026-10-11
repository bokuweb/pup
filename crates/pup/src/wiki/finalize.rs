//! `pup wiki-finalize`: regenerate every machine block from the published data, roll back event
//! pages the LLM did not write, advance the state, and rebuild `index.md` and `log.md`.

use super::facts::Facts;
use super::plan::{skeleton_market, skeleton_type, Plan, State};
use super::*;
use crate::config::{EVENT_TYPES, MARKETS};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value = "public/data", env = "WIKI_BASE")]
    pub base: String,
    #[arg(long, default_value = "wiki")]
    pub wiki: PathBuf,
    #[arg(long, default_value = ".wiki-work")]
    pub work: PathBuf,
}

pub fn run(a: &Args) -> Result<()> {
    let facts = Facts::new(Source::new(&a.base)?);
    let mut state = State::load(&a.wiki);
    let plan: Option<Plan> = std::fs::read(a.work.join("plan.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let mut done = Vec::new();
    if let Some(plan) = &plan {
        for t in &plan.tasks {
            let p = a.wiki.join(&t.event_page);
            let written = Page::read(&p)
                .map(|pg| !pg.llm().trim().is_empty())
                .unwrap_or(false);
            if written {
                state.processed.insert(t.event_id.clone(), t.stage.clone());
                done.push(t);
            } else if t.created.contains(&t.event_page) {
                let _ = std::fs::remove_file(&p);
                tracing::warn!("wiki-finalize: {} not written; rolled back", t.event_id);
            } else {
                tracing::warn!("wiki-finalize: {} not updated; will retry", t.event_id);
            }
        }
    }

    // Event pages that exist, by event id.
    let mut event_pages: BTreeMap<String, (String, String, String)> = BTreeMap::new(); // id → (path, date, title)
    for path in walk_md(&a.wiki.join("events")) {
        let r = rel(&a.wiki, &path);
        let mut page = Page::read(&path)?;
        let id = page.get_str("event_id").to_string();
        if let Some(ev) = facts.event_json(&id)? {
            page.set_block("facts", &render::event_block(&ev));
            page.write(&path)?;
        }
        event_pages.insert(
            id,
            (
                r,
                page.get_str("date").to_string(),
                page.get_str("title").to_string(),
            ),
        );
    }

    for t in EVENT_TYPES {
        let path = a.wiki.join(type_path(t.id));
        let has_events = event_pages
            .keys()
            .any(|id| id.starts_with(&format!("{}-", t.id)));
        if !path.exists() && !has_events {
            continue;
        }
        let mut page = if path.exists() {
            Page::read(&path)?
        } else {
            skeleton_type(t.id)
        };
        let Some(tj) = facts.type_json(t.id)? else {
            continue;
        };
        let from = type_path(t.id);
        let mut list: Vec<(String, String, String)> = event_pages
            .iter()
            .filter(|(id, _)| id.starts_with(&format!("{}-", t.id)))
            .map(|(_, (p, d, title))| (d.clone(), title.clone(), link(&from, p)))
            .collect();
        list.sort_by(|x, y| y.0.cmp(&x.0));
        page.set_block("aggregates", &render::type_block(&tj, &list));
        page.write(&path)?;
    }

    for m in MARKETS.iter().filter(|m| m.wiki) {
        let path = a.wiki.join(market_path(m.id));
        if !path.exists() && event_pages.is_empty() {
            continue;
        }
        let mut page = if path.exists() {
            Page::read(&path)?
        } else {
            skeleton_market(m.id)
        };
        let Some(mj) = facts.source().get(&format!("markets/{}.json", m.id))? else {
            continue;
        };
        let from = market_path(m.id);
        let links = |id: &str| event_pages.get(id).map(|(p, _, _)| link(&from, p));
        page.set_block("reactions", &render::market_block(&mj, &links, 40));
        page.write(&path)?;
    }

    write_index(&a.wiki, &event_pages)?;
    write_log(&a.wiki, &done)?;
    state.save(&a.wiki)?;
    tracing::info!(
        "wiki-finalize: {} tasks written, {} event pages",
        done.len(),
        event_pages.len()
    );
    Ok(())
}

fn write_index(
    wiki: &Path,
    event_pages: &BTreeMap<String, (String, String, String)>,
) -> Result<()> {
    let mut s = String::from("# 経済イベントと市場の反応\n\n重要な経済イベントの前後で、主要な指数がどういう状態からどう動いたかの事実と分析。数値はすべて機械で計算した事実データに照合済み。\n\n## イベントの種類\n\n");
    for t in EVENT_TYPES {
        if wiki.join(type_path(t.id)).exists() {
            s.push_str(&format!("- [{}]({})\n", t.name, type_path(t.id)));
        }
    }
    s.push_str("\n## 市場\n\n");
    for m in MARKETS.iter().filter(|m| m.wiki) {
        if wiki.join(market_path(m.id)).exists() {
            s.push_str(&format!("- [{}]({})\n", m.name, market_path(m.id)));
        }
    }
    let topics: Vec<String> = walk_md(&wiki.join("topics"))
        .iter()
        .map(|p| rel(wiki, p))
        .collect();
    if !topics.is_empty() {
        s.push_str("\n## 論点\n\n");
        for t in topics {
            let title = Page::read(&wiki.join(&t))
                .map(|p| p.get_str("title").to_string())
                .unwrap_or_else(|_| t.clone());
            s.push_str(&format!("- [{title}]({t})\n"));
        }
    }
    let mut recent: Vec<&(String, String, String)> = event_pages.values().collect();
    recent.sort_by(|a, b| b.1.cmp(&a.1));
    s.push_str("\n## 最近のイベント\n\n");
    for (p, d, title) in recent.into_iter().take(30) {
        s.push_str(&format!("- {d} [{title}]({p})\n"));
    }
    Page::new(
        vec![
            ("type", json!("index")),
            ("title", json!("経済イベントと市場の反応")),
            ("description", json!("イベント・市場・論点ページの一覧")),
        ],
        s,
    )
    .write(&wiki.join("index.md"))
}

fn write_log(wiki: &Path, done: &[&super::plan::Task]) -> Result<()> {
    let path = wiki.join("log.md");
    let mut page = Page::read(&path).unwrap_or_else(|_| {
        Page::new(
            vec![
                ("type", json!("log")),
                ("title", json!("更新ログ")),
                ("description", json!("wiki の日次更新の記録")),
            ],
            "# 更新ログ\n".into(),
        )
    });
    if done.is_empty() {
        return page.write(&path);
    }
    let today = today_jst().to_string();
    let mut entry = format!("## {today}\n\n");
    for t in done {
        entry.push_str(&format!(
            "- [{}]({}){}\n",
            t.title,
            t.event_page,
            if t.followup {
                "（20 本目までを追記）"
            } else {
                ""
            }
        ));
    }
    // Replace today's section if this ran twice today; newest first under the heading.
    let marker = format!("## {today}\n");
    if let Some(i) = page.body.find(&marker) {
        let end = page.body[i + marker.len()..]
            .find("\n## ")
            .map(|j| i + marker.len() + j + 1)
            .unwrap_or(page.body.len());
        page.body.replace_range(i..end, &entry);
    } else {
        let head_end = page
            .body
            .find('\n')
            .map(|i| i + 1)
            .unwrap_or(page.body.len());
        page.body.insert_str(head_end, &format!("\n{entry}"));
    }
    page.write(&path)
}
