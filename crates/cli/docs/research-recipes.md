# Research recipes over prep tables

## Motivating question

How much did my agents spend per day, what woke them, which messages rebuilt a cold cache, which models a model router actually ran, and where did the fleet loop, fan out or compact — without writing SQL from scratch?

## Run a recipe

Every recipe reads the canonical views of a `unisphere prep` target. Unisphere links no query engine and never runs one; it prints a self-contained script that you pipe into the external DuckDB CLI:

```sh
unisphere prep --target $TARGET
unisphere prep recipes --human
unisphere prep recipe daily --target $TARGET | duckdb
```

The printed script is exactly three parts: `SET file_search_path = '<absolute TARGET>';`, `.read '<absolute TARGET>/views.sql'`, then the recipe query, so it reads exactly the committed Parquet parts named by `views.sql`. `unisphere prep recipes --json` lists every name with the question it answers; an unknown name exits 2 with the valid names. The script goes to stdout in every output mode; diagnostics go to stderr.

**Install DuckDB first.** If your shell reports `duckdb: command not found` (exit 127), install the DuckDB CLI — `brew install duckdb` on macOS, or the binary from <https://duckdb.org/docs/installation> — and re-run the same pipe. The recipes were validated with DuckDB 1.5.

## Recipes

| Recipe | Answers | Rows |
|---|---|---|
| `daily` | brief Q1 | Per local day: sessions, calls, token classes, cold calls, compactions, peer and human turns |
| `hourly` | brief Q2 | Per local hour: calls, sessions, token classes, cold calls and their cache writes, peer messages |
| `call-timeline` | brief Q3 | Every canonical call in order with seat, context, other-TTL cache writes and native TTL, gap, turn, opener and cold flags |
| `turn-cost` | brief Q4 | Every turn: opener, sender, calls, token classes (including `cw_other`), first-call gap and context, cold rebuild |
| `trigger-kinds` | brief Q5 | Per turn origin: turns, calls, median calls per turn, token classes, cold wakes |
| `sender-attribution` | brief Q6 | Per peer sender: turns opened, recipient sessions, turns of at most 3 calls, token classes, cold wakes |
| `compactions` | brief Q7 | Every compaction: trigger, pre/post tokens, duration, idle gap, first context after |
| `context-bands` | brief Q8 | Per 100k context band: calls, sessions, token classes, cold calls, cache reads above 200k |
| `idle-wakes` | brief Q9 | Turn-opening calls after the cache TTL, largest cache rewrite first |
| `limit-events` | brief Q10 | Limit notices (session, weekly, other) with reset phrase and instant, and auto-continuations |
| `meter-replay` | brief Q11 | Cumulative token classes per 10-minute bucket from the week start |
| `fan-outs` | brief Q12 | One sender, one message hash, many seats within 120 s |
| `message-graph` | brief Q13 | Per (from_seat, to_seat): peer messages, turns opened, cold wakes, token classes |
| `hidden-requests` | brief Q14 | Recaps and compactions: requests absent from usage records |
| `structure` | brief section 4 | Calls, merged duplicates, turns, compactions, recaps, limit notices, cold calls, idle wakes, mean context |
| `trigger-records` | brief section 4 | Trigger records by kind, including those that opened no turn |
| `idle-tax` | fleet: idle tax | Per seat: idle wakes and the cache rewrites they paid |
| `compaction-cadence` | fleet: compaction cadence | Per session: compactions, hours between them, pre-tokens, context after |
| `peer-amplification` | fleet: peer amplification | Per (from_seat, to_seat): turns opened, short turns, token classes, cold wakes |
| `subagent-fanout` | fleet: subagent fan-out | Per parent session: sidechains and their token classes against the parent's |
| `tool-loops` | fleet: tool loops | The same tool and input hash repeated at least 3 times in one turn |
| `fleet-idle` | fleet: idle longest | Main sessions ordered by time since their last native event |
| `expensive-turns` | fleet: most expensive turns | The 50 turns with the most tokens, with the native offset of their opener |
| `daily-tokens` | fleet: daily tokens | Per local day, harness, project (repository) and model: calls and token classes (including `cw_other`) |
| `routed-turns` | routers: which models ran | Every routing decision: turn, selected (router) model, pattern, rule, route source, planned phases and models, the phases that ran in order with their declared models (and the models that actually served them when they differ) and verdicts, the decision's served models and mismatch flag, final model, outcome, native totals, cost, latency and duration |
| `routed-cost` | routers: actual versus selected model | Per harness, selected model and model that ran (routed or not, main or sidechain): calls, token classes, `cw_other`, native cache-write TTLs, output and native cost with its unit |

## Reading the results

- **Token classes, not prices.** Recipes return `input`, `cache_read`, `cw_1h`, `cw_5m` (cache writes) and `output`; `call-timeline`, `turn-cost`, `daily-tokens` and the router recipes also return `cw_other`, cache writes whose native TTL is neither 1 h nor 5 m (Copilot's GPT models record 1800 s). No price is applied; multiply by your own rates. Where a ranking is needed, recipes order by `total_tokens`, the sum of every class.
- **Native cost is not money.** `native_cost` is the harness's own cost counter in `native_cost_unit` (Copilot: `nano_aiu`, nano AI units); it is summed as recorded and never converted to a currency.
- **Null is unrecorded.** A dialect that does not record a counter leaves it null; sums skip nulls and never invent zeros.
- **Derived flags.** `context = input + cache_read + cw_1h + cw_5m + cw_other`; `cache_write` is the three cache-write classes. `cold`: context above 20000 with cache writes at least half of it. `ttl_s`: the call's native `cache_write_ttl_s` when recorded, else 300 for sidechains and 3600 for main sessions. `pred_cold`: the gap since the previous call exceeds `ttl_s`. An idle wake is `cold AND pred_cold`; a rebuild after compaction or restart is `cold AND NOT pred_cold`. `gap_s` is -1 for a source's first call.
- **Routers.** `model` is always the model that ran; `selected_model` is the router's synthetic model (e.g. `hydrafusion`) on routed calls and null otherwise, so `coalesce(selected_model, model)` is what the session selected. Route and phase totals are the router's own native counters; they can include requests the router attributes to a decision without tagging the call (Copilot counts a subagent request made during a phase), so compare them with calls rather than adding them to calls.
- **Seats.** A seat is the session's pij seat hint when recorded, else its session id. Peer senders come from the turn opener.
- **Local time.** Days, hours and buckets use DuckDB's `TimeZone` setting, your local zone by default. Pin it with `duckdb -cmd "SET TimeZone = 'UTC'"`.
- **Current generation only.** Every recipe reads the `*_v` views, so rotated, truncated or rewritten sources never double count.
- **Metadata only.** Recipes return counts, tokens, times, ids, hashes and native offsets, never message text. Fetch one opener with `unisphere prep record --target DIR --source SOURCE --offset OPENER_OFFSET --include-content`.
- **Windows.** Recipes cover the whole target. Scope a study with `prep --modified-since`, or save the script and add a `WHERE ts_ms >= …` filter; `meter-replay` starts seven days before the latest call unless you edit its `week_start_ms`.

## Examples

Every `unisphere` command below is parsed by the test suite with `$TARGET` bound; the part after `|` is the external engine.

```sh
unisphere prep recipes --json
unisphere prep recipe idle-tax --target $TARGET | duckdb
unisphere prep recipe daily-tokens --target $TARGET | duckdb -csv
unisphere prep recipe expensive-turns --target $TARGET | duckdb -json
unisphere prep recipe routed-turns --target $TARGET | duckdb
unisphere prep recipe routed-cost --target $TARGET | duckdb -csv
unisphere prep recipe hourly --target $TARGET | duckdb -cmd "SET TimeZone = 'UTC'"
unisphere prep recipe meter-replay --target $TARGET > meter-replay.sql
```

**Next step:** run `unisphere prep recipes --human`, then `unisphere prep recipe NAME --target DIR | duckdb`.
