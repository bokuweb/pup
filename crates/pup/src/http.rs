use anyhow::{bail, Context, Result};
use std::time::Duration;

pub fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; pup/0.1; +https://github.com/bokuweb/pup)")
        .timeout(Duration::from_secs(60))
        .build()?)
}

/// GET with a few retries on 429/5xx and transport errors.
pub fn get_text(client: &reqwest::blocking::Client, url: &str) -> Result<String> {
    let mut last = None;
    for attempt in 0..4 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
        }
        match client.get(url).send() {
            Ok(resp) if resp.status().is_success() => {
                return resp.text().with_context(|| format!("read {url}"))
            }
            Ok(resp) if resp.status().as_u16() == 429 || resp.status().is_server_error() => {
                last = Some(anyhow::anyhow!("GET {url}: HTTP {}", resp.status()));
            }
            Ok(resp) => bail!("GET {url}: HTTP {}", resp.status()),
            Err(e) => last = Some(anyhow::Error::new(e).context(format!("GET {url}"))),
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("GET {url} failed")))
}

/// Plain text of an HTML page: tags removed, common entities decoded, whitespace collapsed.
pub fn html_text(html: &str) -> String {
    let no_script = regex::Regex::new(r"(?is)<(script|style)[^>]*>.*?</(script|style)>")
        .unwrap()
        .replace_all(html, " ");
    let no_tags = regex::Regex::new(r"<[^>]+>")
        .unwrap()
        .replace_all(&no_script, " ");
    let decoded = no_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&#8209;", "-")
        .replace("&ndash;", "-")
        .replace("&#8211;", "-")
        .replace(['\u{2011}', '\u{2013}'], "-");
    regex::Regex::new(r"\s+")
        .unwrap()
        .replace_all(&decoded, " ")
        .trim()
        .to_string()
}
