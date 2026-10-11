//! `pup wiki-export`: the wiki as JSON for the SPA (`index.json`, `graph.json`, `page/{path}.json`).
//! Event pages without analysis are left out; event-type and market pages always have tables.

use super::*;
use regex::Regex;
use serde_json::json;
use std::collections::BTreeSet;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value = "wiki")]
    pub wiki: PathBuf,
    #[arg(long, default_value = "public/wiki")]
    pub out: PathBuf,
}

const HUBS: [&str; 2] = ["index.md", "log.md"];

pub fn resolve(from: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

pub fn run(a: &Args) -> Result<()> {
    if a.out.exists() {
        std::fs::remove_dir_all(&a.out)?;
    }
    std::fs::create_dir_all(a.out.join("page"))?;
    let mut pages = Vec::new();
    for p in walk_md(&a.wiki) {
        let r = rel(&a.wiki, &p);
        match Page::read(&p) {
            Ok(pg) if !(pg.get_str("type") == "event" && pg.llm().trim().is_empty()) => {
                pages.push((r, pg))
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("wiki-export: skip {r}: {e:#}"),
        }
    }
    let known: BTreeSet<String> = pages.iter().map(|(r, _)| r.clone()).collect();
    let link_re = Regex::new(r"\]\(([^)\s#]+)").unwrap();
    let mut index = Vec::new();
    let mut nodes = Vec::new();
    let mut links = BTreeSet::new();
    for (r, pg) in &pages {
        let id = r.trim_end_matches(".md");
        let fm: serde_json::Map<String, Value> = pg.frontmatter.iter().cloned().collect();
        index.push(json!({
            "path": id, "type": pg.get_str("type"), "title": pg.get_str("title"), "description": pg.get_str("description"),
            "date": fm.get("date"), "tags": fm.get("tags"), "event_id": fm.get("event_id"), "event_type": fm.get("event_type"),
            "market": fm.get("market"), "has_analysis": !pg.llm().trim().is_empty(),
        }));
        let p = a.out.join("page").join(format!("{id}.json"));
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(
            &p,
            serde_json::to_vec(&json!({"path": id, "frontmatter": fm, "body": pg.body}))?,
        )?;
        if HUBS.contains(&r.as_str()) {
            continue;
        }
        nodes.push(json!({"id": id, "type": pg.get_str("type"), "title": pg.get_str("title")}));
        for c in link_re.captures_iter(&pg.body) {
            if let Some(t) = resolve(r, &c[1]) {
                if known.contains(&t) && !HUBS.contains(&t.as_str()) && &t != r {
                    let (x, y) = if r.as_str() < t.as_str() {
                        (r.clone(), t)
                    } else {
                        (t, r.clone())
                    };
                    links.insert((
                        x.trim_end_matches(".md").to_string(),
                        y.trim_end_matches(".md").to_string(),
                    ));
                }
            }
        }
    }
    std::fs::write(a.out.join("index.json"), serde_json::to_vec(&index)?)?;
    let links: Vec<Value> = links
        .into_iter()
        .map(|(s, t)| json!({"source": s, "target": t}))
        .collect();
    std::fs::write(
        a.out.join("graph.json"),
        serde_json::to_vec(&json!({"nodes": nodes, "links": links}))?,
    )?;
    tracing::info!("wiki-export: {} pages", index.len());
    Ok(())
}
