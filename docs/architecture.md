# Architecture

pup separates what must be exact from what needs judgment, the same way lawrenceanum does.

| Layer | Owner | Output |
|---|---|---|
| Inputs | `pup collect` (deterministic) | `data/` — daily closes and event calendars (not committed) |
| Facts | `pup build` (deterministic) | `public/data/` — event windows, aggregates, reactions |
| Prose | Claude Code (Sonnet 5.5) | the `<!-- llm:begin -->` blocks of wiki pages |
| Checks | `pup wiki-check` (deterministic) | pass/fail; a failing day is not pushed |
| App | Vite + React SPA | GitHub Pages |

The facts on Pages are the source of truth. The wiki (orphan branch `wiki`) is derived and can be
deleted and rebuilt.

## Wiki (OKF)

[Open Knowledge Format](https://agentic-ai.readthedocs.io/en/latest/Standards/open-knowledge-format/):
Markdown with a YAML frontmatter whose required field is `type`. Values are JSON literals, one key
per line, so the frontmatter is valid YAML and is read and written without a YAML parser.

```
wiki/
├── index.md                    type: index  (generated)
├── log.md                      type: log    (generated, one section per day)
├── events/{type}/{date}.md     type: event       facts (generated) + analysis (LLM)
├── event-types/{type}.md       type: event-type  aggregates (generated) + trends and record (LLM)
├── markets/{id}.md             type: market      reactions (generated) + trends and record (LLM)
├── topics/{name}.md            type: topic       written by the LLM when a pattern spans events
└── .pup/state.json             events written and at which stage (r5 / r20)
```

Generated blocks are `<!-- pup:begin NAME -->` … `<!-- pup:end NAME -->`; `wiki-finalize`
rewrites them from the published facts on every run.

### Stages

An event is written up once five closes have passed (`r5`) and revisited once twenty have (`r20`),
when the agent rewrites only its "その後" section. Events are taken newest first, up to
`WIKI_MAX_ITEMS` per day, three per Claude Code session.

### Source bundles

`wiki-plan` gives the agent, per event: the release's facts, every market's pre-event state and
moves (full for the wiki markets, `r1`/`r5`/`r20` for the rest), the aggregates of the sub-groups the
event belongs to, and the previous four releases of the same type. The agent reads nothing else.

### Fact scopes and checks

| Page | May cite |
|---|---|
| event | its own event id, `type-{its type}` |
| event-type | `type-{type}`, every event of that type |
| market, topic | every event id, every `type-*` |

`wiki-check` verifies, for each number with a unit in an LLM block: a footnote in the same clause;
the footnote's scope is allowed on the page; the fact exists; the footnote repeats the fact's text;
the number equals it (a range fact may be cited by either end); a direction word after the number
agrees with the fact's sign (levels such as rates are exempt). It also validates frontmatter types
and relative links. Matching a number against any fact on the page is too weak — an event carries
about two hundred facts and a wrong number often equals one of them — so numbers are tied to their
footnotes.

## Cost

A local trial with Sonnet 5.5 took 23 turns and about $0.90 for one event (the event page, the
event-type page and five market pages). At the default of four events a day that is roughly
$3–4 a day on the API, or the equivalent usage on a Pro/Max plan with `CLAUDE_CODE_OAUTH_TOKEN`.
Lower `WIKI_MAX_ITEMS` to slow the backlog (about 180 events since 2016) or the spend.
