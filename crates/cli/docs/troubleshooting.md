# Error recovery

Errors contain a stable code/category, fixed safe explanation, structural location when available, `retryable`, and a typed recovery action. They never echo raw hostile arguments, parser text, source payload, or source stderr.

| Code | Cause | Safe recovery and retry condition |
|---|---|---|
| `UNI-QUERY-ARGUMENT` | Invalid command/option/value combination or mixed native/query selectors. | Read the relevant `--help`; choose a listed command/option. Retry after correcting grammar. |
| `UNI-CLI-PIJ-ID` | Empty, option-like, whitespace-bearing, or control-bearing Pij seat ID. | Copy one explicit seat ID from the intended Pij instance; rejected text is not echoed. |
| `UNI-CLI-PIJ-CONFLICT` | `--pij` was combined with a native source/session identity. | Keep `--pij ID` as the only identity selector, or remove it and use explicit native selectors. |
| `UNI-QUERY-FIELD` | Field is undeclared for this dataset. | Run `unisphere schema show DATASET --json`, then choose a returned field. |
| `UNI-QUERY-PATTERN` | Invalid or over-bound glob/regex. | Correct or narrow the pattern; do not retry unchanged. |
| `UNI-QUERY-TIME` | Ambiguous/invalid time. | Use RFC3339 or `YYYY-MM-DD` UTC; retry after replacing the value. |
| `UNI-QUERY-SOURCE-MISSING` | Explicit source is absent in admitted scope. | Rediscover it or supply an existing explicit source. |
| `UNI-QUERY-SOURCE-READ` | Permission/read consistency failure. | Change permissions or wait for source stability, then open a fresh view. |
| `UNI-QUERY-SOURCE-UNSUPPORTED` | Representation/dialect unsupported. | Choose a registered adapter alternative; conversion is not automatic. |
| `UNI-QUERY-SCHEMA` | Saved input is not a supported versioned query document. | Regenerate supported JSON/JSONL; native data belongs to native export/loaders. |
| `UNI-QUERY-OPERATION` | Dataset/format/metric/group cannot perform the operation. | Inspect schema capabilities and choose a listed operation/format. |
| `UNI-QUERY-DATA` | Supplied evidence violates the typed contract. | Regenerate valid data; do not mutate source history inside this query. |
| `UNI-QUERY-IDENTITY` | Identity resolves to conflicting candidates. | Select one returned local candidate ID. |
| `UNI-QUERY-BRANCH` | Branch/membership is ambiguous. | Select one returned branch ID; no branch is chosen by recency. |
| `UNI-QUERY-CURSOR` | Options/view changed or token is invalid. | Repeat original options on unchanged evidence, or start fresh without cursor. |
| `UNI-QUERY-FIELD-MISSING` | Offline/in-process view lacks a required field. | Supply versioned input retaining returned required fields. |
| `UNI-QUERY-INPUT-SUBSET` | Saved rows cannot establish complete context/partition. | Supply complete versioned JSON extraction; no implicit live enrichment. |
| `UNI-QUERY-VIEW-SCOPE` | Request widens admitted scope/capability. | Explicitly reopen a source view with the needed scope/fields. |
| `UNI-QUERY-CONTENT-CONSENT` | Sensitive output requested without consent. | Choose metadata-only columns or deliberately add `--include-content`. |
| `UNI-QUERY-LIMIT` | Named resource bound exceeded. | Narrow selection or configure a supported explicit bound; never silently truncate. |
| `UNI-QUERY-OUTPUT` | Destination/pipe failed; bytes may be partial. | Discard partial output, choose a new writable destination, then retry. |

Pij resolution failures distinguish an unavailable optional executable, daemon/auth/transport failure, unknown seat, known seat without a recorded native session, unsupported harness, missing local source, invalid response, bounds, and timeout. Follow the emitted cause-specific action. Resolution never installs, starts, or revives Pij, searches a federated roster, fetches a remote transcript, or guesses from process/pane state.

## Unknown docs topics and fields

`unisphere docs list --json` returns every valid topic. An unknown topic should name those IDs and suggest `docs list`; it never performs fuzzy remote lookup. `schema show` is the sole field/operation authority. An unknown live adapter/harness is rejected before source reads and names registered alternatives.

## Outcome-specific next actions

Success suggests an actual next workflow step; zero matches suggests inspecting scope/coverage; partial results suggest inspecting coverage or narrowing; help/version suggest a starting topic; completed exports suggest validating the consumer; errors name the prerequisite that must change. JSON envelopes carry `next_action`; data streams use diagnostics. Suggestions never run automatically.

For JSONL and CSV, coverage, result-universe completeness, format losses, and next actions are separate JSON diagnostics on stderr. CSV collapses absent, null, and empty values into empty cells; use JSON or JSONL when those states must remain distinct. `--csv-safety raw` leaves formula-like strings uninterpreted by Unisphere and therefore unsafe to open directly in spreadsheet software. A zero-row stream with subset, unknown, or incomplete coverage is not a complete zero-result claim.

**Next step:** follow the recovery attached to the actual error code rather than retrying unchanged.
