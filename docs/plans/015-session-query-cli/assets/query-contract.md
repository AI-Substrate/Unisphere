# Shared query contract — preferred direction, not implemented

## Why one contract comes before many commands

The user wants to answer questions about agent work, not learn a different data model for each harness. Finding a session, extracting its failed commands and computing latency must refer to the same entities and the same selected evidence. If one command counts files, another counts native IDs, and a third counts replayed events, the tool gives contradictory answers.

This product plan specifies observable semantics. The future PM owns the reviewed Rust interfaces, final crate layout, sealed contracts, work packets and execution proof. No CLI fleet is authorised yet. Plan014 owns Git-ai note ingestion; this plan must not change that source contract in parallel. Retired harness telemetry is excluded entirely.

## Queryable datasets

| Dataset | Unit of observation | Why users need it | What it must not pretend |
|---|---|---|---|
| sources | A native store/representation and its observed revision or watermark | Diagnose discovery and provenance; understand why a session is missing | A file or index row is not automatically a new session |
| sessions | An evidence-associated agent conversation/run, with zero or more source representations | Find past work, inspect lineage, choose a conversation | Native IDs are not globally unique; an attribution note does not prove a transcript exists |
| turns | A source-supported request/response episode with associated messages and calls | Extract meaningful pieces of work, not arbitrary physical lines | One JSONL record is not a turn; progress messages and compaction are not extra user turns |
| messages | A normalised message with a source role and content-policy projection | Read/search user requests and assistant responses | Tool payloads must not silently become ordinary assistant prose |
| tools | One invocation, optionally paired to one or more result/progress records | Find commands, failures and observed timings | A start/result pair is not two calls; progress is not a terminal result |
| events | A normalised source-derived event with physical provenance | Debug projections and handle details not represented by higher-level views | An event is not automatically an invocation, model inference or OTel span |

Common row fields: `id`, `native_id` (nullable), `harness`, `source_refs`, timestamp fields, and `availability` for missing/partial/derived fields. IDs minted by Unisphere are explicitly local/query identities; they never populate `gen_ai.conversation.id` when the source supplied no conversation ID. Source-only records remain visible in sources/events when session association cannot be established.

Session rows additionally expose name/model observations, project associations, lineage, transcript availability, and observed counts with completeness qualifiers. Message/turn/tool rows link back through explicit IDs. A session may have multiple models; a session-model filter matches any observed model, while a tool/message-model filter matches the model attributed to that row. Missing model attribution is not inherited speculatively.

## Scope and discovery

Live dataset queries require an explicit `--repo PATH` or `--source PATH_OR_ID`. Offline queries take `--input FILE|-`. Do not silently combine live and offline evidence, use the current directory as a surprise scan root, or contact remote services. `--repo .` is an explicit request to inspect registered local source locations for this repository; it is not authority to recursively read arbitrary HOME content.

`--repo-scope exact|tree|worktrees` defaults to `tree`: exact repo directory and component-boundary descendants. `exact` excludes descendant working directories. `worktrees` additionally includes Git-verified linked worktrees. Similar prefixes, equal basenames, repository mentions inside prompts and remote URLs alone do not prove project association. Known lossy directory encodings are labelled hints; they cannot silently override contradictory native metadata. Source discovery exposes unreadable, unassociated, unsupported and absent locations separately.

`--source` accepts explicit known source files/stores or a source ID returned by discovery. New source dialects are registered in the existing catalogue; no shadow registry. On query commands, `--harness` selects the human-facing harness family and `--source-adapter` selects a concrete representation; there is no query `--adapter` alias. Thus Cursor transcript and IDE forms can share a harness without becoming interchangeable adapters. The SDK derives typed adapter/harness source selection before provider I/O; excluded registrations are not scanned or decoded and cannot force failure or partial-read acceptance.

### Native and query command routing

One CLI-owned root parser produces typed commands for app dispatch; raw first-token/default-adapter scanning is removed. `sessions export` is always native export. `sessions list --root PATH` remains the existing bounded native JSONL-file listing. `sessions list --adapter git-ai --repo PATH` remains the existing native Git-note listing; legacy `--adapter` is a native dispatch selector, not a query predicate. All other `sessions list/show/tree/stats/extract` query forms use `--repo`, `--source` or saved `--input`, with `--source-adapter` for representation filtering. For example, `sessions list --repo . --source-adapter git-ai` is a logical query, distinct from native note inventory.

Reject mixed native/query selector forms rather than guessing a mode. On query forms `--repo` means repository association scope; on explicitly native Git-note list/export it selects the Git repository. Existing native invocations retain their semantic data and stream placement, with additive next-action metadata; OTLP bytes stay unchanged. Suggested actions and compatibility checks use this same real parser grammar.

## Identity, lineage and deduplication

Keep repository identity, harness, native conversation identity and subagent identity separate. A Claude subagent can reuse a parent's session ID; native ID alone cannot collapse them. Track known parent/fork relationships with their evidence; cycles, dangling references and missing parents are reported rather than repaired by guesswork.

Merge source copies only when stable source/native evidence proves they represent the same entity. Preserve all supporting source references. Do not content-hash equal user text into one request. A retry is a distinct invocation when it has distinct source identity; a copied/forked prefix is not newly consumed usage merely because another file contains it. Conflicting identities remain explicit and are excluded from an allegedly exact aggregate unless the user chooses a documented unresolved policy.

Deduplication/reconciliation is a read-view operation over the selected sources. This plan does not promise exactly-once durable ingestion, retained historical revisions or source finality. Snapshot adapters supply a current replacement projection; that is not a history of every prior state.

## Turns and tool pairing

Prefer native turn/request IDs and explicit terminal boundaries. Where a supported format requires reconstruction, version and label the rule: one initiating user request plus its associated assistant/tool activity up to the next initiating request or an explicit terminal event. Tool-result messages, user-shaped tool responses, injected context and compaction records are not new initiating requests merely because a physical role field says `user`. Fork branches are selected explicitly, never concatenated as if chronological neighbours proved ancestry. Unresolvable fragments remain events with `turn_id: null`; turn counts state their observed basis.

Recognized source formats may supply a versioned query membership rule from a validated session header, native request containment or explicit parent tree. This is evidence-qualified query reconstruction, not a change to record-local OTLP mapping or permission to invent a missing native conversation ID. Source-only or unresolved membership remains explicitly unavailable.

Pair calls/results by supported native identity within the correct session/branch scope, not adjacency or command text. Preserve original tool name and a separately registered tool family such as `shell` or `file-read`. Multiple shell implementations are discoverable under `--tool-family shell`; original names remain available. Unknown tool families remain unknown.

A duration is either source-reported or derived from a valid matched start/end pair with compatible clocks; carry `duration_basis`. Missing completion, clock reversal and ambiguous pairing produce null duration plus a reason, never zero. Distinguish `succeeded`, `failed`, `cancelled`, `incomplete` and `unknown`. Missing exit code does not mean success. Overlapping calls mean summed durations are not wall-clock session time.

## Filtering, time and ordering

- Different filter fields combine with AND; repeated values of the same field combine with OR. Exclusion flags subtract after positive selection. Unknown values do not satisfy a positive comparison.
- `--contains` is literal text search, `--regex` is explicit regex search, and `--name` is a glob over recorded names. Default case-sensitive; `--ignore-case` is explicit. Reject invalid patterns. Bound regex/input work; no user code evaluation or SQL/shell interpolation.
- Name/branch/model/role filters apply only where evidence exists. Unsupported filter fields fail with a schema/capability explanation rather than returning a misleading empty set.
- `--since` is inclusive and `--until` exclusive. Accept RFC3339 timestamps and `YYYY-MM-DD` (UTC midnight). Reject ambiguous locale dates and natural-language relative dates. Compare instants, not strings.
- Default time fields: sessions `started_at`, turns `started_at`, messages/events `timestamp`, tools `started_at`. `--time-field` chooses another declared field. `first_event_at` is separate from native start; file mtime is never activity. `--include-undated` explicitly retains unknown timestamps, with unknowns sorted last.
- Session-time selection is not event-time selection. Messages from a month-old session may match today's event window. Selecting tools by start time does not imply their full interval lies inside the window.
- Stable default order: sessions descending selected timestamp; turns/messages/tools/events ascending selected timestamp; sources by stable source ID. Tie-break by stable entity ID. Missing timestamps sort last in both directions; do not invent cross-source causality from tie-break order.
- `--sort FIELD` is ascending; `--sort=-FIELD` descending. `--columns` projects declared fields only. Invalid fields name the relevant `schema show` command.
- `--range 12:20` on turns requires exactly one unambiguous session and branch/view. It is inclusive, one-based and applied to stable full-view turn ordinals before other row filters; filtered rows are not renumbered.

List defaults to 50 rows in every output format, not a TTY-dependent change of meaning. `--limit 0` means all rows subject to explicit safety bounds. Pagination continuation is tied to query options and source revisions/watermarks; changed evidence must reject a stale token or require a new query, never silently skip/duplicate rows. The implementation guide must choose the exact token mechanism without requiring a daemon. Stats scan the full matched set before limiting output groups; extract defaults to all matched rows within its declared resource bounds.

## Extraction and content privacy

`list` browses a projected dataset; `show` expands one selected entity; `extract` emits reusable selected data/content; `stats` reduces the same semantic dataset. Existing `sessions export --input FILE` remains native-source to OTLP LogsData, not an alias for Markdown extraction.

`--context-before N` / `--context-after N` apply to turns/messages after matching, partitioned by each matched row's admitted session/branch. Multiple unambiguous session/branch groups are valid in one repository-wide extraction; only `--range` requires exactly one group. Merge overlapping windows within each group, label rows as `match` or `context`, and report matched versus emitted counts. Context may expand beyond date filters deliberately, but never beyond the admitted repository/source/participant authority. Linear text/Markdown uses separate labeled groups rather than inventing one cross-branch timeline. Actual ambiguous selectors or unresolved membership fail with valid branch alternatives; other datasets reject context flags.

Metadata is not anonymity. Default output omits recorded session titles/names, message bodies, command strings, tool arguments/results, reasoning, human identity strings and free-form source attributes. A recorded name can itself contain prompt text. Payload-search flags (including `--name`) authorise inspecting their named content locally, but do not authorise emitting it. `--include-content` is required for payload-bearing projections, names, snippets, text/Markdown extraction and tool `--part input|output|all`. Without it, a requested sensitive column fails clearly rather than leaking or pretending it was empty. Select only explicitly supported content; never follow external URLs, sidecars or attachments implicitly.

Always escape terminal controls and untrusted Markdown/HTML appropriately. CSV defaults to `--csv-safety spreadsheet`: string cells whose first non-whitespace character is `=`, `+`, `-` or `@`, or which begin with tab/CR, receive a leading apostrophe before normal RFC4180 encoding. Numeric cells remain numeric. `--csv-safety raw` explicitly preserves string values and warns about spreadsheet interpretation; it does not bypass content consent. CSV is a convenience projection, not a lossless round-trip format: null is an empty cell, empty strings can be indistinguishable to CSV consumers, and compound values are JSON text. Use JSON/JSONL when those distinctions matter. No blanket 'redacted' or 'secret-free' claim: omission policies require synthetic canary proof for every adapter and output mode. Reading content locally must not cause it to be persisted in an index, query log, diagnostic or report.

The schema explicitly marks CSV as lossy for projected absence, null and empty strings, and CSV diagnostic guidance points to JSON/JSONL when those distinctions matter. The typed SDK/JSON availability contract is not a CSV round-trip promise.

`--output FILE` is create-new by default; no overwrite flag in this initial scope. Use temporary staging/atomic publication where needed to avoid presenting a partial file as a complete extract. For stdout, a later error may leave partial bytes; exit/report must say incomplete. No successful checkpoint/final manifest after failed output. Pipe closure is handled explicitly and tested.

## Output contracts

- Human tables are presentation, not a machine parser contract. Data goes to stdout; warnings/progress/coverage summaries to stderr.
- JSON query output uses the existing command envelope style: `ok`, `command`, `v: 1`, `data` containing `schema_version: 1`, `dataset`, `query`, `rows`, `coverage`, `universe`, `matched`, `emitted` and `next_cursor` where applicable, plus top-level `next_action`.
- JSONL is one versioned typed row per line, suitable for `jq`/DuckDB; no interleaved banners. Common reserved fields are `schema_version`, `dataset`, `id`, and source provenance. Declared data fields such as `duration_ms` are directly addressable. Summary/coverage goes to stderr or an explicitly requested separate manifest, not a fake data row.
- CSV is a flat UTF-8 RFC4180 projection with a header, the null/compound encoding and safety modes above, and explicit time-unit column names. Text/Markdown are supported by session/turn/message extraction, and payload-focused tool extraction, not arbitrary metrics serialization.
- `schema show DATASET` exposes fields, types, nullability, units, content sensitivity, valid filters, default time/sort rules, capability limits and format losses. It is the discoverable contract used by humans and agents.
- Query JSON/JSONL is a view format, not a replacement for Unisphere's standard OTLP telemetry output. Never masquerade it as LogsData. OTLP export retains pinned standard semantics; Unisphere-specific concepts remain versioned extensions.
- Offline input accepts supported versioned query JSON/JSONL, not arbitrary native formats. A projected input missing fields needed for a requested operation fails with an availability reason. Never enrich it by silently rescanning live stores.
- Preserve existing `--json` and `--human` behavior. New query commands additionally accept `--format`; specifying more than one output-mode selector is an argument error even if the modes appear equivalent. No hidden precedence or TTY-dependent change to selected rows. Docs/config keep their existing-style mode surface unless their documented parser explicitly adds another format.

Retain existing exit conventions: 0 for completed query (including zero matches), 1 for operational/read/output failure, 2 for invalid arguments. An unreadable requested source is not an empty success. `--allow-partial` is the only way to accept partial source reads; output explicitly carries incomplete coverage, including in machine-mode summaries. Optional missing fields alone are not read failures.

### Saved input and completeness

Saved JSON records source-view, selection and column digests, projected columns, applied limit, and separate `rows_complete_for_selection` and `partitions_complete` states. Capturing every filtered match does not prove that all neighbouring rows in a source/session/branch were saved. A present continuation or fewer emitted than matched rows prevents an all-matches claim; emitted context rows do not establish full partitions either. Validate metadata consistency and required fields rather than trusting EOF.

Standalone query JSONL has no completeness proof merely because it ends at a newline. Treat it as provided rows with unknown universe completeness unless an explicit validated completeness record is supplied; never add a fake data row or implicitly open a sidecar. Filtering/list/show can operate on the available rows. Statistics over incomplete saved inputs are explicitly bounded by those provided rows, not exact totals for the original query or source. Offline context/reconstruction requires complete needed partitions and order/membership fields; otherwise return actionable input-availability failure instead of silently shortening context. Recover with a complete versioned JSON extraction, never hidden live enrichment.

An in-process query view also retains its admitted scope, source selection, fields and universe. A subsequent query may narrow that view; widening it or requiring unavailable fields must request a new view/input rather than return a misleading empty result.

### Next actions and actionable errors

Every command outcome must tell the user or calling agent what to try next: success, zero matches, partial results, help, version and completed exports, not just failures. Machine command envelopes carry a nonempty `next_action`; human presentation labels the next step. Raw JSONL, CSV, text/Markdown extracts and OTLP remain clean data: next-action guidance belongs in their documented summary/diagnostic channel, never an extra data record or trailer. This applies to existing config/catalog/list/export commands as well as the new query/docs/schema surface.

Choose the action from the actual outcome: inspect a selected entity, follow a continuation, narrow a broad result, diagnose an empty source set, inspect coverage, or use the next recipe step. Recommendations must use supported command/option grammar and preserve live/offline scope and content consent. They are suggestions, never automatically executed actions. Use safe known identifiers where available; when required input cannot safely be reproduced, name the missing input explicitly rather than leaking it or presenting a placeholder as an executable command.

Every error names a stable category/code, explains the failure safely and gives a concrete cause-specific recovery step. Name valid alternatives for invalid commands/options/topics/fields; distinguish missing, unreadable, unsupported and malformed input; explain ambiguous identity/branch selection, stale continuations, denied content projection, resource bounds and output failures. State whether retry can help and what must change first. Do not merely repeat the error, advise blind retries, echo hostile arguments/content or automatically relax privacy and safety bounds.

An output failure must not report completed output, advance a checkpoint or add a repair envelope to already-partial data. Emit actionable diagnostics on a healthy diagnostic channel where possible; if that destination also fails, retain a nonzero exit and make no claim that guidance was delivered.

Proof must exercise all command leaves plus help/version, legitimate zero matches and partial outcomes; parse emitted command suggestions with the real parser after binding explicitly named inputs, and execute representative next steps. Fault cases cover the error categories above, safe stream routing and sentinel content. Presence of a `next_action` key or a generic help link alone does not prove useful guidance.

## Statistics are evidence, not arithmetic over whatever is present

Use the same identity/dedup rules as list/extract. Report total invocations, measured durations, missing durations and each outcome separately. Duration mean/percentiles use only valid measured samples; empty measured set produces null statistics. Specify percentile method (nearest-rank for this contract), units and rounding. Failure rate uses known terminal outcomes and names its denominator; incomplete/unknown calls are not counted as successes.

Token buckets remain source-qualified (per-call, cumulative session, cache read/write). Do not sum cumulative snapshots, replayed prefixes or incompatible token semantics. No inferred monetary cost: a future cost estimator would require explicit versioned pricing and an estimate label. Grouping by harness/model is descriptive, not a causal comparison of agent quality. Tool names, skill tags and nominal durations are not proof of productive work.

## Resource and future-PM decisions

Queries must have explicit bounds for source count, input bytes/record size, decoded rows, sort/aggregation memory, regex work and output bytes. Reuse existing loader bounds where appropriate; the guide must name defaults and prove refusal boundaries. No silent truncation or hidden approximate percentiles. Stream where ordering permits; materialise only when the operation requires it.

A persistent derived index is not required for this first contract. The PM must measure representative repositories before proposing one; if justified, it is rebuildable, opt-in and separate from authoritative sources and user-curation state. No daemon, service, hosted database or model service is mandatory. Persistent resume, exactly-once ingestion, remote collection, automatic repair, agent replay/execution, source deletion, annotations and model-based summarisation remain out of scope.
