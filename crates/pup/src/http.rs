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

/// GET bytes with the same retries as [`get_text`].
pub fn get_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let mut last = None;
    for attempt in 0..4 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
        }
        match client.get(url).send() {
            Ok(resp) if resp.status().is_success() => {
                return Ok(resp
                    .bytes()
                    .with_context(|| format!("read {url}"))?
                    .to_vec())
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

/// GET text that may be gzip-compressed without saying so: the Internet Archive's raw (`id_`)
/// snapshots return the original gzip bytes with no `Content-Encoding`.
pub fn get_text_gunzip(client: &reqwest::blocking::Client, url: &str) -> Result<String> {
    let bytes = get_bytes(client, url)?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut out)
            .with_context(|| format!("gunzip {url}"))?;
        Ok(out)
    } else {
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// Text of a PDF's first page via poppler's `pdftotext` (`layout` keeps table rows on one line).
pub fn pdf_first_page(pdf: &[u8], layout: bool) -> Result<String> {
    let dir = std::env::temp_dir().join(format!("pup-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("in.pdf");
    std::fs::write(&path, pdf)?;
    let mut cmd = std::process::Command::new("pdftotext");
    if layout {
        cmd.arg("-layout");
    }
    let out = cmd
        .args(["-l", "1"])
        .arg(&path)
        .arg("-")
        .output()
        .context("run pdftotext (install poppler-utils)")?;
    anyhow::ensure!(out.status.success(), "pdftotext failed");
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Full-width digits and signs to ASCII; `▲`/`△` (used for negatives in Japanese tables) to `-`.
pub fn ascii_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap(),
            '．' => '.',
            '－' | '▲' | '△' | '−' => '-',
            c => c,
        })
        .collect()
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
