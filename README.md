# pup

How major market indices behaved around key economic events — the state they were in before,
and how they moved after — as machine-computed facts, charts, and an OKF wiki in which Claude
writes the analysis and every number it writes is checked against those facts.

- **Events**: FOMC decisions (2016–), Bank of Japan policy meetings (2016–), US CPI and the US
  employment report (with a FRED API key).
- **Markets**: S&P 500, Nasdaq 100, Russell 2000, VIX, Nikkei 225, TOPIX (1306 ETF), DAX, Euro Stoxx 50,
  FTSE 100, Hang Seng, US 10-year yield, USD/JPY, dollar index, gold, WTI, bitcoin.
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
                                   FRED release dates + BLS figures (CPI/NFP, only with FRED_API_KEY)
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

Every number with a unit the LLM writes (`%`, `bp`, `pt`, `千人`) needs a footnote in the same
clause, `[^n]: fact:{scope}/{key} {text}`, where the fact exists in the published data, the footnote
repeats its text, the number equals it, and a direction word after it (上昇/下落/…) agrees with its
sign. Computing new numbers (differences, ratios) is not allowed. One failure blocks the day's push.

## Setup

1. **Pages**: Settings → Pages → Source: *GitHub Actions*.
2. **Secrets**
   - `CLAUDE_CODE_OAUTH_TOKEN` (`claude setup-token`, runs on a Pro/Max plan) or `ANTHROPIC_API_KEY`.
   - `FRED_API_KEY` (optional, free at <https://fred.stlouisfed.org/docs/api/api_key.html>) to add
     US CPI and employment releases.
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
- BLS figures are the currently published (revised) values, not the first print.
- Release-time conventions: FOMC times come from the statement; CPI/NFP are 08:30 New York; BOJ
  decisions are taken as 12:00 Tokyo (marked as approximate).
- Consensus forecasts are not included: the free calendars that show them redistribute licensed
  vendor data, so "surprise" is approximated by the change from the previous release.
- Not investment advice.
