//! `pup wiki-plan`: pick events to write up (newest first, capped), write a compact source bundle
//! per event and create page skeletons. An event is planned once five sessions have passed (`r5`)
//! and again when twenty have (`r20`) so the write-up can add what happened next.

use super::facts::Facts;
use super::*;
use crate::config::{self, MARKETS};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(clap::Args)]
pub struct Args {
    /// Published data: `public/data` or the Pages URL of it.
    #[arg(long, default_value = "public/data", env = "WIKI_BASE")]
    pub base: String,
    #[arg(long, default_value = "wiki")]
    pub wiki: PathBuf,
    #[arg(long, default_value = ".wiki-work")]
    pub work: PathBuf,
    #[arg(long, default_value_t = 6, env = "WIKI_MAX_ITEMS")]
    pub max_items: usize,
    /// Events per Claude Code invocation.
    #[arg(long, default_value_t = 3)]
    pub chunk_size: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    /// event id → stage written (`r5` / `r20`).
    pub processed: BTreeMap<String, String>,
}

impl State {
    pub fn load(wiki: &Path) -> State {
        std::fs::read(wiki.join(STATE_PATH))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, wiki: &Path) -> Result<()> {
        let p = wiki.join(STATE_PATH);
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(p, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Task {
    pub event_id: String,
    pub kind: String,
    pub title: String,
    pub date: String,
    pub stage: String,
    pub followup: bool,
    pub bundle: String,
    pub event_page: String,
    pub type_page: String,
    pub market_pages: Vec<String>,
    pub created: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Plan {
    pub date: String,
    pub tasks: Vec<Task>,
}

fn compact(facts: &Value) -> Vec<Value> {
    facts
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| json!({"key": f["key"], "label": f["label"], "text": f["text"]}))
        .collect()
}

pub fn skeleton_event(ev: &Value) -> Page {
    let e = &ev["event"];
    let title = e["title"].as_str().unwrap_or_default();
    let mut p = Page::new(
        vec![
            ("type", json!("event")),
            ("title", json!(title)),
            ("description", json!("")),
            ("date", e["date"].clone()),
            ("event_type", e["type"].clone()),
            ("event_id", e["id"].clone()),
            ("tags", json!([])),
        ],
        format!("# {title}\n\n<!-- pup:begin facts -->\n<!-- pup:end facts -->\n\n## 分析\n\n{LLM_BEGIN}\n{LLM_END}\n"),
    );
    p.set_block("facts", &render::event_block(ev));
    p
}

pub fn skeleton_type(kind: &str) -> Page {
    let t = config::event_type(kind);
    let name = t.map(|t| t.name).unwrap_or(kind);
    Page::new(
        vec![("type", json!("event-type")), ("title", json!(name)), ("description", json!(t.map(|t| t.description).unwrap_or(""))), ("event_type", json!(kind)), ("tags", json!([]))],
        format!("# {name}\n\n<!-- pup:begin aggregates -->\n<!-- pup:end aggregates -->\n\n## 傾向と記録\n\n{LLM_BEGIN}\n{LLM_END}\n"),
    )
}

pub fn skeleton_market(id: &str) -> Page {
    let name = config::market(id).map(|m| m.name).unwrap_or(id);
    Page::new(
        vec![("type", json!("market")), ("title", json!(name)), ("description", json!("")), ("market", json!(id)), ("tags", json!([]))],
        format!("# {name}\n\n<!-- pup:begin reactions -->\n<!-- pup:end reactions -->\n\n## 傾向と記録\n\n{LLM_BEGIN}\n{LLM_END}\n"),
    )
}

pub fn run(a: &Args) -> Result<()> {
    let facts = Facts::new(Source::new(&a.base)?);
    let state = State::load(&a.wiki);
    if a.work.exists() {
        std::fs::remove_dir_all(&a.work)?;
    }
    std::fs::create_dir_all(a.work.join("sources"))?;
    std::fs::create_dir_all(a.work.join("chunks"))?;
    let events = facts.events()?;

    let mut tasks = Vec::new();
    for e in events.iter() {
        if tasks.len() >= a.max_items {
            break;
        }
        let (Some(id), Some(stage)) = (e["id"].as_str(), e["stage"].as_str()) else {
            continue;
        };
        let done = state.processed.get(id).map(String::as_str);
        let followup = match (done, stage) {
            (None, _) => false,
            (Some("r5"), "r20") => true,
            _ => continue,
        };
        let Some(ev) = facts.event_json(id)? else {
            continue;
        };
        let kind = ev["event"]["type"].as_str().unwrap_or_default().to_string();
        let date = ev["event"]["date"].as_str().unwrap_or_default().to_string();
        let tj = facts.type_json(&kind)?.unwrap_or(Value::Null);
        let groups: Vec<String> = ev["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g[0].as_str().map(String::from))
            .collect();

        // Aggregates for this event's sub-groups, wiki markets, horizons 1/5/20 (keeps the bundle small).
        let wiki_markets: Vec<&str> = MARKETS.iter().filter(|m| m.wiki).map(|m| m.id).collect();
        let type_facts: Vec<Value> = compact(&tj["facts"])
            .into_iter()
            .filter(|f| {
                let k = f["key"].as_str().unwrap_or_default();
                let parts: Vec<&str> = k.split('.').collect();
                parts.len() == 5
                    && groups.iter().any(|g| g == parts[1])
                    && wiki_markets.contains(&parts[2])
                    && ["r1", "r5", "r20"].contains(&parts[3])
            })
            .collect();
        let markets: Vec<Value> = ev["markets"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|m| {
                let mid = m["market"].as_str().unwrap_or_default();
                let full = config::market(mid).is_some_and(|d| d.wiki);
                let fs: Vec<Value> = compact(&m["facts"])
                    .into_iter()
                    .filter(|f| {
                        full || f["key"].as_str().is_some_and(|k| {
                            k.ends_with(".r1") || k.ends_with(".r5") || k.ends_with(".r20")
                        })
                    })
                    .collect();
                json!({"market": mid, "name": config::market(mid).map(|d| d.name), "facts": fs})
            })
            .collect();
        // The previous releases of the same type, for context (titles and a few reactions).
        let previous: Vec<Value> = events
            .iter()
            .filter(|x| x["type"] == kind.as_str() && x["date"].as_str() < Some(date.as_str()))
            .take(4)
            .map(|x| json!({"id": x["id"], "title": x["title"], "facts": compact(&x["facts"])}))
            .collect();
        let bundle_rel = format!("{}/sources/{id}.json", a.work.display());
        let event_page = event_path(&kind, &date);
        let type_page = type_path(&kind);
        let market_pages: Vec<String> = wiki_markets.iter().map(|m| market_path(m)).collect();
        let bundle = json!({
            "event_id": id, "title": ev["event"]["title"], "type": kind, "date": date, "time_utc": ev["event"]["time_utc"],
            "time_approx": ev["event"]["time_approx"], "stage": stage, "followup": followup,
            "scopes": [id, format!("type-{kind}")],
            "event_facts": compact(&ev["event"]["facts"]),
            "markets": markets,
            "type_facts": type_facts,
            "previous_same_type": previous,
            "pages": {"event": event_page, "type": type_page, "markets": market_pages},
        });
        std::fs::write(
            a.work.join("sources").join(format!("{id}.json")),
            serde_json::to_vec_pretty(&bundle)?,
        )?;

        let mut created = Vec::new();
        let ep = a.wiki.join(&event_page);
        if !ep.exists() {
            skeleton_event(&ev).write(&ep)?;
            created.push(event_page.clone());
        }
        let tp = a.wiki.join(&type_page);
        if !tp.exists() {
            skeleton_type(&kind).write(&tp)?;
        }
        for (m, p) in wiki_markets.iter().zip(&market_pages) {
            if !a.wiki.join(p).exists() {
                skeleton_market(m).write(&a.wiki.join(p))?;
            }
        }
        tasks.push(Task {
            event_id: id.into(),
            kind,
            title: ev["event"]["title"].as_str().unwrap_or_default().into(),
            date,
            stage: stage.into(),
            followup,
            bundle: bundle_rel,
            event_page,
            type_page,
            market_pages,
            created,
        });
    }

    let plan = Plan {
        date: today_jst().to_string(),
        tasks,
    };
    std::fs::write(a.work.join("plan.json"), serde_json::to_vec_pretty(&plan)?)?;
    let describe = |t: &Task| {
        format!(
            "### {}\n- イベント: `{}`（{}{}）\n- ソースバンドル: `{}`\n- 書くページ: `wiki/{}`（分析）、`wiki/{}`（傾向と記録）、{}（記録に 1 行）\n",
            t.title,
            t.event_id,
            t.stage,
            if t.followup { "、追記: 20 本目までの「その後」" } else { "" },
            t.bundle,
            t.event_page,
            t.type_page,
            t.market_pages.iter().map(|p| format!("`wiki/{p}`")).collect::<Vec<_>>().join("・")
        )
    };
    let mut md = format!("# wiki plan {}\n\n{} 件\n\n", plan.date, plan.tasks.len());
    for t in &plan.tasks {
        md.push_str(&describe(t));
    }
    std::fs::write(a.work.join("plan.md"), &md)?;
    for (i, chunk) in plan.tasks.chunks(a.chunk_size.max(1)).enumerate() {
        let mut c = format!("# タスク（チャンク {}）\n\n", i + 1);
        for t in chunk {
            c.push_str(&describe(t));
        }
        std::fs::write(a.work.join("chunks").join(format!("{:03}.md", i + 1)), c)?;
    }
    tracing::info!("wiki-plan: {} tasks", plan.tasks.len());
    Ok(())
}
