//! `pup wiki-check`: OKF frontmatter, relative links, and every number in LLM-written text.

use super::facts::{check_llm, Facts};
use super::*;
use regex::Regex;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value = "public/data", env = "WIKI_BASE")]
    pub base: String,
    #[arg(long, default_value = "wiki")]
    pub wiki: PathBuf,
    #[arg(long, default_value = ".wiki-work")]
    pub work: PathBuf,
    /// Only pages changed in the wiki working tree (git status).
    #[arg(long)]
    pub changed: bool,
}

fn changed_pages(wiki: &Path) -> Result<Vec<PathBuf>> {
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(wiki)
        .output()?;
    if !out.status.success() {
        return Ok(walk_md(wiki));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.get(3..))
        .map(|p| p.trim_matches('"'))
        .filter(|p| p.ends_with(".md"))
        .map(|p| wiki.join(p))
        .filter(|p| p.exists())
        .collect())
}

/// Scopes whose facts a page may cite.
fn scopes(facts: &Facts, page: &Page) -> Result<Vec<String>> {
    let events = facts.events()?;
    let ids_of = |kind: Option<&str>| -> Vec<String> {
        events
            .iter()
            .filter(|e| kind.is_none_or(|k| e["type"] == k))
            .filter_map(|e| e["id"].as_str().map(String::from))
            .collect()
    };
    let all_types = || {
        crate::config::EVENT_TYPES
            .iter()
            .map(|t| format!("type-{}", t.id))
            .collect::<Vec<_>>()
    };
    Ok(match page.get_str("type") {
        "event" => vec![
            page.get_str("event_id").to_string(),
            format!("type-{}", page.get_str("event_type")),
        ],
        "event-type" => {
            let k = page.get_str("event_type");
            let mut v = vec![format!("type-{k}")];
            v.extend(ids_of(Some(k)));
            v
        }
        "market" | "topic" => {
            let mut v = all_types();
            v.extend(ids_of(None));
            v
        }
        _ => vec![],
    })
}

pub fn run(a: &Args) -> Result<()> {
    let facts = Facts::new(Source::new(&a.base)?);
    let pages = if a.changed {
        changed_pages(&a.wiki)?
    } else {
        walk_md(&a.wiki)
    };
    let link_re = Regex::new(r"\]\(([^)\s]+)\)").unwrap();
    let mut report = Vec::new();
    for path in &pages {
        let r = rel(&a.wiki, path);
        let page = match Page::read(path) {
            Ok(p) => p,
            Err(e) => {
                report.push(format!("- `{r}`: {e:#}"));
                continue;
            }
        };
        let t = page.get_str("type");
        if !TYPES.contains(&t) {
            report.push(format!(
                "- `{r}`: frontmatter `type` must be one of {TYPES:?} (got {t:?})"
            ));
        }
        if page.get_str("title").is_empty() {
            report.push(format!("- `{r}`: frontmatter `title` is empty"));
        }
        for c in link_re.captures_iter(&page.body) {
            let target = c[1].split('#').next().unwrap_or_default();
            if target.is_empty() || target.starts_with("http://") || target.starts_with("https://")
            {
                continue;
            }
            let resolved = path.parent().unwrap_or(Path::new(".")).join(target);
            if !resolved.exists() {
                report.push(format!("- `{r}`: broken link `{target}`"));
            }
        }
        let llm = page.llm();
        if llm.trim().is_empty() {
            continue;
        }
        let sc = scopes(&facts, &page)?;
        let allowed = facts.allowed(&sc)?;
        for e in check_llm(&llm, &facts, &sc, &allowed)? {
            report.push(format!("- `{r}`: {e}"));
        }
    }
    std::fs::create_dir_all(&a.work)?;
    let md = if report.is_empty() {
        format!("## wiki-check\n\n{} ページ: 問題なし\n", pages.len())
    } else {
        format!(
            "## wiki-check\n\n{} ページ中 {} 件の問題\n\n{}\n",
            pages.len(),
            report.len(),
            report.join("\n")
        )
    };
    std::fs::write(a.work.join("check-report.md"), &md)?;
    print!("{md}");
    if !report.is_empty() {
        bail!("wiki-check: {} problems", report.len());
    }
    Ok(())
}
