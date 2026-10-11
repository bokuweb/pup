# pup

How major market indices behaved around key economic events — the state they were in before,
and how they moved after — as machine-computed facts, charts, and an OKF wiki in which Claude
writes the analysis and every number it writes is checked against those facts.

- **Events**: FOMC decisions, Bank of Japan policy meetings, ECB Governing Council decisions,
  Bank of England MPC decisions, Japan's national CPI, the BOJ Tankan, US CPI, the US employment
  report, US PPI, JOLTS, advance retail sales, the GDP advance estimate and the PCE price index
  (all 2016–).
- **Markets**: S&P 500, Nasdaq 100, Russell 2000, VIX, Nikkei 225, TOPIX (1306 ETF), DAX, Euro Stoxx 50,
  FTSE 100, Hang Seng, US 10-year yield, USD/JPY, EUR/USD, GBP/USD, dollar index, gold, WTI, bitcoin.
- **App**: GitHub Pages (`https://bokuweb.github.io/pup/`) — event charts (±20 sessions), averages
  with interquartile bands by event type and sub-group, reactions per market, and the wiki.

The architecture follows [lawrenceanum](https://github.com/bokuweb/lawrenceanum): deterministic
Rust for data and checks, Claude Code in GitHub Actions for prose only, the wiki on an orphan
branch, and a static SPA on Pages. See [docs/architecture.md](docs/architecture.md).

## How the facts are defined

For a market and an event released at instant `t`:

- **Before** is the last daily close strictly before `t`. A 08:30 New York release is measured from
  the previous US close; Tokyo's reaction to a 14:00 New York FOMC statement is the next Tokyo close.
- **After N** is the N-th close after `t` (N = 1, 2, 5, 10, 20), measured from the "before" close:
  percent change for indices, FX and commodities; points for VIX; basis points for the 10-year yield.
- **Pre-event state**: change over the 5 and 20 sessions before, 20-session volatility, distance from
  the 52-week high, gap to the 200-day average, and levels for VIX and the yield.
- **Aggregates** per event type and sub-group (FOMC hike/hold/cut, CPI up/down vs the prior month,
  unemployment up/down, VIX regime before the event): mean, median, quartiles, share of rises.

Only derived values are published; raw price histories are not.

## Pipeline

```
update-data.yml  (daily, JST 07:00)
  pup collect   → data/            Yahoo Finance daily closes, Fed statements, BOJ meeting dates,
                                   ECB decisions + key rates (Data Portal), BoE MPC pages + Bank Rate,
                                   Japan CPI summaries (e-Stat) and Tankan summaries (BOJ), read
                                   with pdftotext (poppler-utils) and cached per release,
                                   US CPI/NFP/PPI/JOLTS release dates (FRED with FRED_API_KEY, otherwise the
                                   Internet Archive's index of archived BLS releases) + BLS figures,
                                   Census advance retail reports (PDF), BEA news archive and
                                   release pages (GDP advance estimate, Personal Income and Outlays)
  pup build     → public/data/     event windows, aggregates, market reactions (JSON)
  pup wiki-export → public/wiki/   wiki branch → JSON
  web (Vite)    → Pages

update-wiki.yml  (daily, JST 08:00)
  pup wiki-plan      pick events (newest first, capped), source bundles + page skeletons
  Claude Code        write analysis from the bundles only (Sonnet 5.5, wiki/ edits only, no Bash/network)
  pup wiki-finalize  regenerate tables, index, log; roll back pages that were not written
  pup wiki-check     OKF frontmatter, links, every number vs its cited fact → one repair pass → push
```

### Number checking

Every number with a unit the LLM writes (`%`, `bp`, `pt`, `千人`, `千件`) needs a footnote in the same
clause, `[^n]: fact:{scope}/{key} {text}`, where the fact exists in the published data, the footnote
repeats its text, the number equals it, and a direction word after it (上昇/下落/…) agrees with its
sign. Computing new numbers (differences, ratios) is not allowed. One failure blocks the day's push.

## Setup

1. **Pages**: Settings → Pages → Source: *GitHub Actions*.
2. **Secrets**
   - `CLAUDE_CODE_OAUTH_TOKEN` (`claude setup-token`, runs on a Pro/Max plan) or `ANTHROPIC_API_KEY`.
   - `FRED_API_KEY` (optional, free at <https://fred.stlouisfed.org/docs/api/api_key.html>). Without
     it, US release dates come from the archived BLS release files.
3. **Variables** (optional): `WIKI_MODEL` (default `claude-sonnet-5-5`), `WIKI_MAX_ITEMS` (default 4).
4. Run *Update data and deploy*, then *Update LLM wiki* (`push_mode: pr` for a trial run).

## Local

```bash
cargo build --release
./target/release/pup collect && ./target/release/pup build      # data/ → public/data/
git init wiki                                                     # wiki-check --changed uses git status
./target/release/pup wiki-plan --max-items 1
claude -p "\`.github/wiki-agent.md\` の指示に従い、\`.wiki-work/chunks/001.md\` の全タスクを処理してください。" \
  --model claude-sonnet-5-5 --allowedTools "Read,Glob,Grep,Edit(wiki/**),Write(wiki/**)" --disallowedTools "Bash,WebFetch,WebSearch"
./target/release/pup wiki-finalize && ./target/release/pup wiki-check --changed
./target/release/pup wiki-export && (cd web && npm ci && npm run dev)
```

## Limits

- Yahoo Finance's chart endpoint is unofficial and may rate-limit; a failed fetch keeps the previous data.
- BOJ meetings carry only the date for now; the decision is published in PDFs and is not parsed yet.
- BLS figures are the currently published (revised) values, not the first print. Retail sales,
  GDP and PCE are first prints, read from each release.
- The BEA archive can lag the latest release by a few weeks; such a release appears once listed.
- Without a FRED key, a US release appears once the Internet Archive has crawled the BLS archive
  file, which can be a few days after the release. The Employment Situation releases delayed by the
  2025 shutdown are mapped to their reference months by hand.
- Release-time conventions: FOMC times come from the statement; CPI/NFP are 08:30 New York; BOJ
  decisions are taken as 12:00 Tokyo (marked as approximate); ECB decisions are 13:45 Frankfurt
  until June 2022 and 14:15 after; BoE decisions are 12:00 London (the emergency cut of 11 March 2020
  is taken as 07:00 and both March 2020 special meetings are marked as approximate).
- Japan's CPI and Tankan figures are as first published (the release summary of each month or
  quarter); CPI y/y rates are levels (`1.9%`), changes are in points (`-0.1pt`), and the Tankan DI
  is written in points (`24pt`). The third CPI headline (excluding fresh food and energy) starts
  with the January 2017 data.
- ECB and BoE decisions are read from the policy rate only (deposit facility rate, Bank Rate);
  asset purchases and other balance-sheet measures are not recorded.
- Consensus forecasts are not included: the free calendars that show them redistribute licensed
  vendor data, so "surprise" is approximated by the change from the previous release.
- Sources: Federal Reserve, Bank of Japan, ECB (reuse with attribution), Bank of England, Statistics
  Bureau of Japan (総務省統計局, Public Data License 1.0 / CC BY 4.0 compatible), Yahoo Finance. Only dates, decisions, rate levels and derived market moves are published.
- Not investment advice.
