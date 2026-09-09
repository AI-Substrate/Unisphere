# Session exploration: workflows and command reference

**Status:** preferred-direction product design. All new commands and outputs below are proposed, not installed capabilities. Existing configuration/catalog/native export behavior is identified separately. Examples use the fully synthetic `synthetic-query-fixture.json`; shown results are contract-relevant projections, not fabricated execution transcripts or a frozen complete wire schema.

## The problem users are trying to solve

Agent work is scattered across harness-specific stores. The user remembers a task, a command or a date—not the filename, database key or physical record layout. They need to find the relevant work, understand it, select useful evidence, and send a bounded result into a review or another program. A tool that implements each noun independently but disagrees on identity, time or privacy makes that job harder.

The operating loop is **learn → discover → select → inspect → extract → measure**. Every step retains enough provenance and coverage information to explain what the result does and does not establish. The SDK must expose the same behavior without shelling out to the CLI.

## Workflow 1: find yesterday's work without knowing the storage layout

**Person:** developer returning to a repository. **Question:** which sessions worked here, and why is one missing?

Start with `unisphere docs get find-sessions`, then `sources list --repo .` and `sessions list --repo .`. Refine with name, harness, model or native/session ID. Use `sources check` when the result is unexpectedly empty. The source view explains unreadable stores and unassociated metadata; it does not encourage blind recursive searches over HOME.

**Decision enabled:** choose a known session or repair the explicit scope. **Success:** the reader can distinguish two physical copies from two separate runs and an unavailable source from a legitimate zero-match query. **Counterexample:** a directory named like the project is not enough to override a contradictory native cwd.

## Workflow 2: reconstruct the context of a failure

**Person:** reviewer or debugger. **Question:** what request led to the failed shell command, and what did the agent do next?

Use `tools list --repo . --status failed`, inspect the call with `tools show`, then extract whole matching turns with `turns extract --has-tool-family shell --has-errors --context-before 1`. Add `--include-content` only when intentionally reviewing command/message payloads.

**Fixture result:** `c-demo-3` is the failed call. `t-demo-2` matches; `t-demo-1` is preceding context. Matched count is1, emitted turns2. **Decision enabled:** distinguish an environmental build failure from an unhelpful request/retry pattern. **Do not infer:** failure cause from exit code alone, or source completeness from the available fragment.

## Workflow 3: review the user's requests within an event-time window

**Person:** someone improving prompts or preparing a handover. **Question:** what did I ask during this day, regardless of when the sessions began?

Use `messages extract --repo . --role user --since 2026-09-01 --until 2026-09-02 --time-field timestamp --include-content --format text`.

**Fixture result:** “Check the parser.” and “Run lint.” The next-day build request is excluded. **Decision enabled:** compare intent, not thousands of tool/progress records. **Pitfall:** filtering session creation dates first would miss today's messages inside an older session. Text search is local inspection permission, not automatic content-output permission.

## Workflow 4: measure shell calls without bogus averages

**Person:** performance investigator. **Question:** which shell calls happened, how long did the measured ones take, and how much timing data is missing?

Use `tools extract --repo . --tool-family shell --include-content --columns id,command,duration_ms,status --format jsonl`, or `tools stats` for the built-in reduction. Pipe structured rows into jq, DuckDB or a plotting program; do not scrape the human table.

**Fixture:** cargo test succeeded in1500ms; cargo clippy failed in500ms; a Codex exec_command has no completion. Three calls, two measured, one missing; measured mean1000ms. Nearest-rank p50=500ms and p95=1500ms. **Decision enabled:** identify slow/repeated/failing operations while seeing measurement coverage. **Do not infer:** incomplete=success, missing=0, sum(duration)=wall time, or a source's cumulative token snapshot=new usage.

## Workflow 5: hand over a bounded conversation slice

**Person:** developer asking a teammate for help. **Question:** can I share only the relevant request/response episodes?

Use `turns extract --repo . --session s-demo-1 --range 1:2 --include-content --format markdown --output review-excerpt.md`. Review the output before sharing. Turn ordinals are stable within the selected session/branch, not renumbered by filters. Existing files are not overwritten. Context outside a date filter is labelled rather than silently smuggled into the result.

**Decision enabled:** share useful context with an auditable boundary. **Pitfall:** metadata can still identify a project/person; content opt-in is not a secret scrubber or publication approval.

## Workflow 6: debug the abstraction instead of trusting it blindly

**Person:** adapter author or support engineer. **Question:** why does this turn/tool count look wrong?

Inspect `schema show tools`, `events show EVENT` and the referenced source revision; compare call identity/pairing and availability. Use `sessions tree` for explicit parent/fork relationships. An unresolved tool result or compaction fragment remains visible as an event, not an invented turn.

**Decision enabled:** identify a mapping/association gap and submit a small synthetic reproduction. **Pitfall:** source copies and forked prefixes can repeat IDs and usage. A rebuildable query view must not treat every physical record as new activity.

## Workflow 7: integrate without adopting a service stack

**Person:** another CLI, report generator or SDK consumer. **Question:** can I query or export these observations inside my own process/pipeline?

Use the Rust SDK's injected query service for in-process work; use JSON/JSONL and `schema show` for command pipelines. Use the existing explicit-source `sessions export` when the consumer wants OTLP LogsData rather than query rows. Saved query output can be queried offline with `--input`; unavailable fields fail clearly rather than triggering hidden rescans.

**Decision enabled:** build a custom report without deploying a daemon, database or LLM service. **Pitfall:** query JSON is not OTLP, and an SDK cursor is not an exactly-once destination transaction.

## Shared syntax and interpretation

The authoritative semantic rules are in [query-contract.md](query-contract.md). First-class docs and their executable proof are in [documentation-design.md](documentation-design.md). The machine-readable catalogue below is [command-catalog.json](command-catalog.json); it is a planning contract, not a second implementation registry. The PM must connect it to real parser/schema/example checks rather than ship a handwritten list that can drift.

### Shared selector families

- Scope: `--repo`, `--repo-scope exact|tree|worktrees`, `--source`, or offline `--input`.
- Identity: `--harness`, `--adapter`, `--session`, `--native-id`, `--model`.
- Time: `--since`, `--until`, `--time-field`, `--include-undated`.
- Text: `--contains`, `--regex`, `--ignore-case`; session names use `--name` glob.
- Output: `--columns`, `--sort`, `--limit`, continuation cursor, `--format`, `--include-content`, `--output`, and explicit partial-read acceptance.
- Dataset-specific selectors: session name/branch/kind/parent/min-turns; turn range/has-role/has-tool/has-tool-family/has-errors/min-tool-calls; message role/turn; tool name/family/status/exit-code/min-duration/has-duration/command-contains; event kind/turn/call.

A command rejects unsupported filter/format combinations rather than pretending every option makes sense everywhere. Existing `--json`/`--human` contracts remain supported; multiple mode selectors, including a new `--format`, are rejected instead of given hidden precedence. CSV defaults to spreadsheet-safe string escaping; explicit `--csv-safety raw` is available with a warning and does not bypass content consent.

## Command reference

Every entry gives the user question, purpose, runnable-shaped example argv, proposed result projection, interpretation and a meaningful limit/error case. `.` means the synthetic demo repository in these examples. New syntax is NOT a claim that today's binary accepts it.

### `unisphere adapters list`

**Question:** Does this release understand my harness?

**Why:** Discover supported formats before scanning private stores.

```sh
unisphere adapters list --json
```

**Expected result projection (synthetic):**

```json
{
  "adapter_ids": [
    "claude-code",
    "codex"
  ],
  "illustration": "subset only; actual registry includes every supported adapter"
}
```

**Interpretation:** Capabilities describe support, not detected installations.

**Failure/limit:** Unsupported source is not guessed from a similar filename.

### `unisphere sources list`

**Question:** Which stores contain evidence for this repo?

**Why:** Diagnose where repository work can be read and attributed.

```sh
unisphere sources list --repo . --format json
```

**Expected result projection (synthetic):**

```json
{
  "rows": [
    {
      "id": "src-demo-1",
      "adapter": "claude-code",
      "format": "jsonl",
      "project_path": "/workspace/demo-flight-bag",
      "association": "native.cwd",
      "read_status": "readable",
      "revision": "synthetic-revision-a"
    },
    {
      "id": "src-demo-2",
      "adapter": "codex",
      "format": "jsonl",
      "project_path": "/workspace/demo-flight-bag",
      "association": "native.cwd",
      "read_status": "readable",
      "revision": "synthetic-revision-b"
    }
  ],
  "coverage": {
    "unreadable": 0,
    "unassociated": 0
  }
}
```

**Interpretation:** Two readable associated source representations; not two sessions by definition.

**Failure/limit:** Unreadable and unassociated sources appear as coverage categories, not silent zeroes.

### `unisphere sources check`

**Question:** Why is a source missing or rejected?

**Why:** Explain empty discovery before users widen scans unsafely.

```sh
unisphere sources check --source src-demo-1 --format json
```

**Expected result projection (synthetic):**

```json
{
  "id": "src-demo-1",
  "read_status": "readable",
  "association": "native.cwd",
  "format": "jsonl"
}
```

**Interpretation:** Read-only structural readiness, not a promise that every source field is known.

**Failure/limit:** An invalid ref/path or permission failure is operational failure, not no sessions.

### `unisphere sessions list`

**Question:** Where is the airspace work?

**Why:** Find prior work by user-facing metadata rather than filename archaeology.

```sh
unisphere sessions list --repo . --name '*Airspace*' --harness claude-code --include-content --format json
```

**Expected result projection (synthetic):**

```json
{
  "rows": [
    {
      "id": "s-demo-1",
      "native_id": "native-session-a",
      "harness": "claude-code",
      "name": "Airspace parser",
      "started_at": "2026-09-01T10:00:00Z",
      "turn_count": 2,
      "message_count": 4,
      "tool_call_count": 3,
      "source_ids": [
        "src-demo-1"
      ]
    }
  ],
  "matched": 1
}
```

**Interpretation:** One logical session after proven representation deduplication.

**Failure/limit:** Names may be absent; ambiguous native IDs cannot select an arbitrary session.

### `unisphere sessions show`

**Question:** What is this run and which evidence supports it?

**Why:** Orient a reader before dumping a large transcript.

```sh
unisphere sessions show s-demo-1 --repo . --include-content --format json
```

**Expected result projection (synthetic):**

```json
{
  "id": "s-demo-1",
  "native_id": "native-session-a",
  "harness": "claude-code",
  "name": "Airspace parser",
  "started_at": "2026-09-01T10:00:00Z",
  "turn_count": 2,
  "message_count": 4,
  "tool_call_count": 3,
  "source_ids": [
    "src-demo-1"
  ]
}
```

**Interpretation:** Observed counts and source links; missing fields remain explicitly unavailable.

**Failure/limit:** Missing transcript does not erase a metadata-only known session.

### `unisphere sessions tree`

**Question:** Did this run delegate, fork or resume?

**Why:** Understand delegation and forks without conflating them.

```sh
unisphere sessions tree s-demo-1 --repo . --format json
```

**Expected result projection (synthetic):**

```json
{
  "root": "s-demo-1",
  "children": [],
  "lineage_coverage": "no child relationship observed in fixture"
}
```

**Interpretation:** No observed children is not proof that the source captured every child.

**Failure/limit:** Cycles/dangling parents are flagged; no parent is inferred from equal text.

### `unisphere sessions stats`

**Question:** How much observed activity is in this repo?

**Why:** Estimate the size and shape of selected work before reading it.

```sh
unisphere sessions stats --repo . --format json
```

**Expected result projection (synthetic):**

```json
{
  "sessions": 2,
  "turns": 3,
  "messages": 6,
  "tool_calls": 4
}
```

**Interpretation:** Observed fixture counts, not a productivity score or full-history guarantee.

**Failure/limit:** Partial or unresolved evidence qualifies totals; cumulative usage is not blindly summed.

### `unisphere sessions extract`

**Question:** Can I take this conversation into a review?

**Why:** Share or archive the selected readable view without hand-copying.

```sh
unisphere sessions extract --repo . --session s-demo-1 --include-content --format markdown
```

**Expected result projection (synthetic):**

```json
{
  "session": "s-demo-1",
  "turn_ids": [
    "t-demo-1",
    "t-demo-2"
  ],
  "message_ids": [
    "m-demo-1",
    "m-demo-2",
    "m-demo-3",
    "m-demo-4"
  ]
}
```

**Interpretation:** Human-readable complete selected view with provenance and uncertainty notes.

**Failure/limit:** Not a raw lossless archive; payload omission without consent is intentional.

### `unisphere sessions export`

**Question:** How do I send native telemetry to my own OTLP importer?

**Why:** Preserve the existing standards-based explicit-source integration.

```sh
unisphere sessions export --adapter claude-code --input /fixtures/native/session-a.jsonl
```

**Expected result projection (synthetic):**

```json
{
  "top_level_key": "resourceLogs",
  "format": "OTLP LogsData JSONL"
}
```

**Interpretation:** Existing operation stays distinct from query-view/Markdown extraction.

**Failure/limit:** Creates new output only; no persisted resume/exactly-once claim.

### `unisphere turns list`

**Question:** Which turns used multiple tools?

**Why:** Navigate requests and their work rather than physical records.

```sh
unisphere turns list --repo . --session s-demo-1 --min-tool-calls 2 --format json
```

**Expected result projection (synthetic):**

```json
{
  "rows": [
    {
      "id": "t-demo-1",
      "session_id": "s-demo-1",
      "ordinal": 1,
      "started_at": "2026-09-01T10:00:00Z",
      "message_ids": [
        "m-demo-1",
        "m-demo-2"
      ],
      "call_ids": [
        "c-demo-1",
        "c-demo-2"
      ],
      "boundary_basis": "synthetic explicit native boundary"
    }
  ],
  "matched": 1
}
```

**Interpretation:** A turn keeps its stable full-view ordinal after filtering.

**Failure/limit:** Unresolvable fragments remain events; no one-line-one-turn fallback.

### `unisphere turns show`

**Question:** What happened after this particular request?

**Why:** Inspect one episode with its associated calls and responses.

```sh
unisphere turns show t-demo-2 --repo . --format json
```

**Expected result projection (synthetic):**

```json
{
  "id": "t-demo-2",
  "session_id": "s-demo-1",
  "ordinal": 2,
  "started_at": "2026-09-01T10:02:00Z",
  "message_ids": [
    "m-demo-3",
    "m-demo-4"
  ],
  "call_ids": [
    "c-demo-3"
  ],
  "boundary_basis": "synthetic explicit native boundary"
}
```

**Interpretation:** One request episode, with links to the failed invocation and messages.

**Failure/limit:** Source boundary and pairing uncertainty are visible.

### `unisphere turns stats`

**Question:** Which sessions contain the most turns?

**Why:** Compare the observed number of request episodes across sessions.

```sh
unisphere turns stats --repo . --group-by session_id --metrics count --sort=-count --format json
```

**Expected result projection (synthetic):**

```json
{
  "rows": [
    {
      "session_id": "s-demo-1",
      "count": 2
    },
    {
      "session_id": "s-demo-2",
      "count": 1
    }
  ]
}
```

**Interpretation:** Counts source-supported turns, not token records or progress messages.

**Failure/limit:** Incomplete boundaries have named coverage, not fabricated complete turns.

### `unisphere turns extract`

**Question:** Give me failed shell turns and the preceding request episode.

**Why:** Keep enough context to understand a failure.

```sh
unisphere turns extract --repo . --has-tool-family shell --has-errors --context-before 1 --include-content --format jsonl
```

**Expected result projection (synthetic):**

```json
{
  "rows": [
    {
      "id": "t-demo-1",
      "selection_role": "context"
    },
    {
      "id": "t-demo-2",
      "selection_role": "match"
    }
  ],
  "matched": 1,
  "emitted": 2
}
```

**Interpretation:** Context expands after matching and stays within the same session branch.

**Failure/limit:** Context may lie outside the filter window and must be labelled.

### `unisphere messages list`

**Question:** What did users actually ask?

**Why:** Remove tool/system noise when reviewing user intent.

```sh
unisphere messages list --repo . --role user --format json
```

**Expected result projection (synthetic):**

```json
{
  "ids": [
    "m-demo-1",
    "m-demo-3",
    "m-demo-5"
  ],
  "content_included": false
}
```

**Interpretation:** Role/identity metadata only unless content is explicitly requested.

**Failure/limit:** Content-search permission does not imply permission to echo text/snippets.

### `unisphere messages show`

**Question:** Show the response referenced by this result.

**Why:** Read one exact message without guessing its location.

```sh
unisphere messages show m-demo-4 --repo . --include-content --format json
```

**Expected result projection (synthetic):**

```json
{
  "id": "m-demo-4",
  "session_id": "s-demo-1",
  "turn_id": "t-demo-2",
  "role": "assistant",
  "timestamp": "2026-09-01T10:02:02Z",
  "text": "Lint failed."
}
```

**Interpretation:** Exact selected message and provenance, not a generated summary.

**Failure/limit:** Unknown or ambiguous ID fails with a selection recovery action.

### `unisphere messages extract`

**Question:** Which user requests were made on September 1?

**Why:** Collect requests from an event-time window for prompt review.

```sh
unisphere messages extract --repo . --role user --since 2026-09-01 --until 2026-09-02 --time-field timestamp --include-content --format text
```

**Expected result projection (synthetic):**

```json
{
  "lines": [
    "Check the parser.",
    "Run lint."
  ],
  "message_ids": [
    "m-demo-1",
    "m-demo-3"
  ]
}
```

**Interpretation:** Filters message timestamps, not session start dates.

**Failure/limit:** Half-open UTC window; undated messages excluded unless explicitly included.

### `unisphere tools list`

**Question:** Which invocations failed?

**Why:** Find operational failures without reading whole conversations.

```sh
unisphere tools list --repo . --status failed --format json
```

**Expected result projection (synthetic):**

```json
{
  "ids": [
    "c-demo-3"
  ],
  "tool_names": [
    "Bash"
  ],
  "exit_codes": [
    101
  ]
}
```

**Interpretation:** One invocation, not separate start and result rows.

**Failure/limit:** Incomplete/unknown calls are not classified as success or failure by absence.

### `unisphere tools show`

**Question:** What was this call and why did it fail?

**Why:** Explain a particular execution and its observed timing.

```sh
unisphere tools show c-demo-3 --repo . --include-content --format json
```

**Expected result projection (synthetic):**

```json
{
  "id": "c-demo-3",
  "session_id": "s-demo-1",
  "turn_id": "t-demo-2",
  "tool_name": "Bash",
  "tool_family": "shell",
  "command": "cargo clippy",
  "started_at": "2026-09-01T10:02:01Z",
  "duration_ms": 500,
  "duration_basis": "matched-source-times",
  "status": "failed",
  "exit_code": 101
}
```

**Interpretation:** Command is inert data; duration carries a measurement basis.

**Failure/limit:** No command execution, URL dereference or attachment loading.

### `unisphere tools stats`

**Question:** How slow are shell calls, and how many are unfinished?

**Why:** Measure the selected calls without distorting missing samples.

```sh
unisphere tools stats --repo . --tool-family shell --metrics count,measured_count,missing_duration_count,failures,mean_ms,p50_ms,p95_ms --format json
```

**Expected result projection (synthetic):**

```json
{
  "count": 3,
  "measured_count": 2,
  "missing_duration_count": 1,
  "failures": 1,
  "mean_ms": 1000,
  "p50_ms": 500,
  "p95_ms": 1500
}
```

**Interpretation:** Nearest-rank percentiles over 1500ms and 500ms; missing duration is not zero.

**Failure/limit:** No measured calls yields null mean/percentiles, not an arithmetic error.

### `unisphere tools extract`

**Question:** List every shell command and its timing for a pipeline.

**Why:** Feed real structured invocations into existing analysis tools.

```sh
unisphere tools extract --repo . --tool-family shell --include-content --columns id,command,duration_ms,status --format jsonl
```

**Expected result projection (synthetic):**

```json
{
  "rows": [
    {
      "id": "c-demo-1",
      "command": "cargo test",
      "duration_ms": 1500,
      "status": "succeeded"
    },
    {
      "id": "c-demo-3",
      "command": "cargo clippy",
      "duration_ms": 500,
      "status": "failed"
    },
    {
      "id": "c-demo-4",
      "command": "cargo test",
      "duration_ms": null,
      "status": "incomplete"
    }
  ]
}
```

**Interpretation:** Three calls remain visible, including the incomplete call with null duration.

**Failure/limit:** Payload columns require content consent; statistics cannot silently drop the missing sample count.

### `unisphere events list`

**Question:** Which tool-start events were observed?

**Why:** Inspect the source-derived substrate when a higher-level view is unclear.

```sh
unisphere events list --repo . --kind tool_start --format json
```

**Expected result projection (synthetic):**

```json
{
  "ids": [
    "e-demo-2",
    "e-demo-4",
    "e-demo-8",
    "e-demo-12"
  ]
}
```

**Interpretation:** Event rows preserve source provenance and do not claim to be complete calls.

**Failure/limit:** Unknown kinds remain named source events rather than invented standard GenAI operations.

### `unisphere events show`

**Question:** What evidence produced this result?

**Why:** Trace an aggregate or projection back to a particular source event.

```sh
unisphere events show e-demo-9 --repo . --format json
```

**Expected result projection (synthetic):**

```json
{
  "id": "e-demo-9",
  "kind": "tool_end",
  "timestamp": "2026-09-01T10:02:01.500Z",
  "session_id": "s-demo-1",
  "call_id": "c-demo-3"
}
```

**Interpretation:** A terminal event supports one pairing, not an independently counted extra invocation.

**Failure/limit:** A missing source revision is reported; never silently read different bytes.

### `unisphere events extract`

**Question:** Extract the September 2 event window.

**Why:** Provide a narrow event slice for another consumer/debugger.

```sh
unisphere events extract --repo . --since 2026-09-02 --until 2026-09-03 --format jsonl
```

**Expected result projection (synthetic):**

```json
{
  "ids": [
    "e-demo-11",
    "e-demo-12",
    "e-demo-13"
  ]
}
```

**Interpretation:** Normalised query-event rows, not OTLP unless using the explicit OTLP export path.

**Failure/limit:** No whole-session completeness inferred from reaching the current EOF.

### `unisphere schema show`

**Question:** Which timing fields can I filter and aggregate?

**Why:** Let people and agents discover fields without reading Rust source.

```sh
unisphere schema show tools --json
```

**Expected result projection (synthetic):**

```json
{
  "dataset": "tools",
  "fields": [
    {
      "name": "duration_ms",
      "type": "number|null",
      "unit": "ms",
      "sensitive": false
    },
    {
      "name": "command",
      "type": "string|null",
      "sensitive": true
    }
  ],
  "illustration": "field subset"
}
```

**Interpretation:** The real response includes all supported fields, filters, units and availability rules.

**Failure/limit:** Unsupported dataset/field is an argument error, not an empty result.

### `unisphere docs list`

**Question:** Where do I start and what can I learn next?

**Why:** Make operating guidance discoverable before any source access.

```sh
unisphere docs list --json
```

**Expected result projection (synthetic):**

```json
{
  "topics": [
    "start",
    "agents",
    "find-sessions",
    "inspect-conversations",
    "filter-time-and-text",
    "extract-context",
    "tool-analysis",
    "output-and-schema",
    "privacy-and-coverage",
    "sdk",
    "troubleshooting",
    "git-ai-notes"
  ]
}
```

**Interpretation:** Topic availability is release-bound; Git-ai page ships only with the actual landed feature.

**Failure/limit:** Works with no stores/Git/Git AI/config/network; not a source-discovery operation.

### `unisphere docs get`

**Question:** How do I analyse shell timing without counting unknown durations as zero?

**Why:** Teach a workflow with interpretation and recovery, not only flags.

```sh
unisphere docs get tool-analysis --human
```

**Expected result projection (synthetic):**

```json
{
  "sections": [
    "Question",
    "Prerequisites",
    "Select shell calls",
    "Interpret duration basis",
    "Aggregate measured samples",
    "Handle missing completions",
    "Related topics"
  ]
}
```

**Interpretation:** One complete version-matched operating page bundled in the installed binary.

**Failure/limit:** Unknown topic names real alternatives and exits nonzero without opening sources.

### `unisphere config check`

**Question:** Is this source configuration valid before I scan anything?

**Why:** Preserve explicit configuration validation without side effects.

```sh
unisphere config check --json
```

**Expected result projection (synthetic):**

```json
{
  "ok": true,
  "command": "config.check",
  "v": 1,
  "data": {
    "configuration": {
      "source_roots": []
    }
  }
}
```

**Interpretation:** Existing configuration behavior remains independent of discovery and collection.

**Failure/limit:** No ambient config lookup or source scan added to config check.

