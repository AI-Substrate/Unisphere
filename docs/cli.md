# Unisphere CLI

Unisphere queries versioned local agent evidence through injected SDK ports,
retains explicit native OTLP export and prepares incremental research tables.
It starts no daemon, remote service, model service or index. Only `prep` reads the
adapter catalogue's default roots under HOME, reports them per set, and
`--no-default-roots` disables that. Query output is not OTLP and snapshot
exports are current replacement projections, not persistent history or source
finality.

## Start offline

The installed binary carries version-matched operating guides and query schemas.
These routes run before configuration, source-provider, Git, or Git-AI setup:

```sh
unisphere --help --human
unisphere --version --json
unisphere docs list --json
unisphere docs get start --human
unisphere schema show sessions --json
```

Every completed command and every safe diagnostic supplies a nonempty next
action. JSON command envelopes use `next_action`. Human output labels `Next:`.
Raw JSONL, CSV, text, Markdown, and OTLP streams contain data only; their action
and coverage summary uses stderr.

## Root grammar

The root parser owns every command. The application dispatches its typed
`ParsedCommand`; it does not inspect raw tokens or choose a default adapter before
parsing.

```text
unisphere adapters list
unisphere config check
unisphere docs list
unisphere docs get TOPIC
unisphere schema show DATASET

unisphere sources list SCOPE [QUERY OPTIONS]
unisphere sources check --source SOURCE [QUERY OPTIONS]

unisphere sessions list SCOPE [QUERY OPTIONS]
unisphere sessions show SESSION SCOPE [QUERY OPTIONS]
unisphere sessions tree SESSION SCOPE [QUERY OPTIONS]
unisphere sessions stats SCOPE [QUERY OPTIONS]
unisphere sessions extract SCOPE [QUERY OPTIONS]
unisphere sessions export NATIVE-SOURCE [NATIVE OPTIONS]

unisphere turns list|stats|extract SCOPE [QUERY OPTIONS]
unisphere turns show TURN SCOPE [QUERY OPTIONS]
unisphere messages list|extract SCOPE [QUERY OPTIONS]
unisphere messages show MESSAGE SCOPE [QUERY OPTIONS]
unisphere tools list|stats|extract SCOPE [QUERY OPTIONS]
unisphere tools show CALL SCOPE [QUERY OPTIONS]
unisphere events list|extract SCOPE [QUERY OPTIONS]
unisphere events show EVENT SCOPE [QUERY OPTIONS]

unisphere prep --target DIR [--root HARNESS[:LABEL]=DIR]... [--harness H]...
    [--no-default-roots] [--include-content] [--max-record-bytes N]
    [--max-batch-bytes N] [--max-snapshot-bytes N] [--max-snapshot-records N]
    [--threads N] [--max-run-bytes N] [--modified-since RFC3339]
unisphere prep compact --target DIR
unisphere prep record --target DIR --source KEY (--offset N | --key K) --include-content
unisphere prep recipes
unisphere prep recipe NAME --target DIR
```

A query scope is exactly one of:

```text
--repo PATH [--repo-scope exact|tree|worktrees]
--source PATH_OR_SOURCE_ID
--input FILE|-
--pij ID
```

Relative paths resolve lexically against the executable-supplied absolute working
directory. The frontend does not use ambient cwd or canonicalize a path while
parsing.

`--input -` reads bounded stdin as versioned query JSON by default. Add
`--stdin-format jsonl` for row-framed query JSONL; `--stdin-format json` is the
explicit default. The flag is rejected with file/live scopes. Unisphere never
sniffs stdin to guess framing. Saved files retain the loader's extension-based
selection.

`--pij ID` is optional and invokes one bounded `pij state ID --json` lookup through
the installed Pij CLI. It accepts live or retired seats with a recorded native
session, verifies that identity through registered local source decoding, and
then uses the ordinary SDK query view. For example:

```sh
unisphere sessions show --pij pij-example-seat --json
unisphere tools extract --pij pij-example-seat --tool-family shell --format jsonl
```

The example seat is a placeholder for your ID. Do not combine `--pij` with another
scope or native session/harness selector. The resolver does not use Pij's cwd hint
to relocate a query, fetch a remote transcript, start a daemon, or combine old
seat incarnations. It emits resolution provenance on stderr using canonical
hashed IDs; continuations use the pinned native source/session rather than
resolving the alias again. Missing Pij, unknown seat, missing recorded native ID,
unsupported protocol/harness and missing local transcript have distinct error
codes and concrete recovery. Ordinary native/query selectors do not require Pij.

Common query selectors include `--harness`, `--source-adapter`, `--session`,
`--native-id`, `--model`, `--name`, `--branch`, `--since`, `--until`,
`--time-field`, `--include-undated`, `--contains`, `--regex`, and
`--ignore-case`. Dataset selectors include turn/message/tool/event identity,
roles, tool names/families/status, duration and observed error fields. Different
fields combine with AND; repeated values in one field combine with OR. Unsupported
field/operation combinations fail with a schema recovery action instead of
returning a misleading empty set.

Projection options are `--columns`, `--sort`, `--limit`, `--cursor`,
`--include-content`, `--allow-partial`, and `--format`. `--format` accepts
`json`, `jsonl`, `csv`, `table`, `text`, or `markdown` only where the selected
dataset operation declares that capability. `--csv-safety spreadsheet` is the
default; `raw` retains potentially active spreadsheet prefixes. CSV collapses
projected absence, null, and empty strings and serializes compound values as JSON
text. Use JSON or JSONL where those distinctions matter.

`--json`, `--human`, and `--format` are mutually exclusive output selectors.
Piped query output defaults to JSON; terminal query output defaults to a table.
TTY status changes presentation only, never row selection.

Queries default to at most **1 GiB of total input**, with separate unchanged
limits of 64 MiB per source, 200,000 observations/rows, 256 MiB retained data and
128 MiB output. A row limit or cursor limits returned results, not source-reading
work; `--allow-partial` does not disable resource limits. These are independent
budgets, not a guarantee that process memory stays below the input allowance.

## Native versus query sessions

These forms have deliberately different meanings:

```sh
# Existing bounded, nonrecursive native JSONL inventory
unisphere sessions list --root /explicit/leaf-project --max-sessions 4096

# Existing native Git-AI note inventory; the registered native adapter owns --repo
unisphere sessions list --adapter git-ai --repo /explicit/repository
unisphere sessions export --adapter git-ai --repo /explicit/repository --include-content

# Logical query; --repo is association scope and --source-adapter is a predicate
unisphere sessions list --repo . --harness claude-code --source-adapter claude-code --format json
unisphere sessions list --repo . --source-adapter git-ai --format json

# Native OTLP export is always native, never query extraction
unisphere sessions export --adapter claude-code --input /explicit/session.jsonl
```

`--adapter` has no query alias. Mixing `--root` or native `--adapter` with query
selectors is `UNI-CLI-ROUTE`; the parser names the three valid alternatives.
`sessions export` remains OTLP JSONL. Query extraction uses the SDK query schema
and never masquerades as `LogsData`.

Native Git Notes list/export also accepts `--notes-ref`, repeated `--commit`,
absolute `--git-executable`, `--max-notes`, `--max-records`,
`--max-note-bytes`, `--max-total-bytes`, `--max-listing-bytes`, and
`--command-timeout-ms`. Defaults remain `refs/notes/ai`, 1,000 notes, 10,000
records, 1 MiB per note/listing, 16 MiB total, and 5,000 ms. These options are
valid only with `--adapter git-ai --repo`; query `--source-adapter git-ai`
remains a representation predicate and does not invoke native Git Notes routing.

Native JSONL and snapshot export accept explicit create-new output paths. Existing
files are not overwritten. Native OTLP bytes stay on stdout or in the new output;
summaries and failures stay on stderr. Query file output is written to a private
same-directory staging file and published through create-new linking only after
serialization and flush complete. A failed stdout writer may contain a prefix;
the nonzero exit and stderr diagnostic state that it is incomplete.

## Privacy and content

Metadata-only output is the default, not anonymity. Names, bodies, commands,
arguments/results, reasoning, identity strings, and free-form native attributes
require the dataset's explicit content capability plus `--include-content`.
Search predicates authorize local inspection of the named content only; they do
not authorize emission. The CLI never inserts rejected raw argument values,
source payloads, or private paths into parse/query recovery text or generated
next actions. When an action requires a private path or original filter, it names
the required input separately instead of inventing or exposing a value.

## Configuration and adapter catalog

```sh
unisphere config check [--config PATH]
    [--source-root ROOT ... | --clear-source-roots] [--json | --human]
unisphere adapters list [--json | --human]
```

Configuration inspection retains defaults → explicit JSON document → explicit CLI
override precedence. It performs no implicit configuration discovery. Repeated
`--source-root` preserves order and duplicates; `--clear-source-roots` replaces
the list with `[]`. The adapter catalog renders only injected registered metadata;
location hints are not installation detection or source discovery.

## Exit and stream contract

| Exit | Meaning |
|---|---|
| 0 | Completed command, including help/version and a legitimate zero-match query |
| 1 | Source, query, serialization, publication, or destination failure |
| 2 | Invalid arguments or an invalid native/query route |
| 3 | `prep` committed but at least one source was unreadable |

JSON command output is one versioned envelope ending in LF. Query JSONL is one
versioned row per line. CSV has one header and row stream. Text/Markdown and OTLP
remain clean data. Operational warnings, partial coverage, raw-stream summaries,
and next actions go to stderr. A diagnostic-channel failure never turns partial
data into success.

## Embedded frontend

`unisphere_cli::parse(Vec<OsString>, &CliContext)` returns:

```text
ParsedCommand::Config | Catalog | Docs | Schema | Query(QueryCommand)
  | NativeRootList | NativeGitNotesList | NativeExport
  | Prep(PrepCommand) | PrepCompact | PrepRecord | PrepRecipes | PrepRecipe
  | Help | Version
```

`QueryCommand` owns a validated `QueryRequest`, output format/CSV policy,
`stdin_format: SavedFormat`, and an optional absolute output path. `run_query`
accepts injected `&dyn QueryApi` and `&dyn QueryWriter` plus caller-owned
stdout/stderr writers. Matching,
reconstruction, privacy authorization, context, continuation, and statistics
remain in `QueryApi`; CLI serialization remains in `QueryWriter`.

Typed execution entrypoints are `run_config`, `run_catalog`, `run_help`,
`run_version`, `run_native_list`, `run_native_export`,
`run_native_snapshot_export`, `run_query`, `emit_query_failure`, `run_docs`,
`run_schema`, `run_prep`, `run_prep_compact`, `run_prep_record`,
`run_prep_recipes`, and `run_prep_recipe`.
`emit_query_failure` keeps initialization failures on the same safe
diagnostic channel without constructing a fake query response. Call
`diagnostic_mode(&args, stdout_is_terminal)` before moving argv into `parse` when
a parse failure must be rendered. These entrypoints consume parsed DTOs and never
inspect argv. The older `run`, `run_adapters`, `run_sessions`, and
`run_snapshot_sessions` APIs remain thin compatibility wrappers around the same
root parser and typed execution. New application composition parses once and
dispatches `ParsedCommand`; `requested_session_adapter` was removed with no
raw-argv compatibility shim.

## Prep canonical tables

`unisphere prep` incrementally folds native sessions into canonical metadata
tables under an explicit target. The composition root adds the adapter
catalogue's default root (label `default`) for every harness admitted by
`--harness`, unless `--no-default-roots`, then every explicit
`--root HARNESS[:LABEL]=DIR`; an unlabelled root gets `root-<8 hex of
sha256(DIR)>`. Sources are keyed `<harness>/<label>/<relative path>`.

```sh
unisphere prep --target /absolute/prep --human
unisphere prep --target /absolute/prep --root claude-code:alt=/home/me/.claude-alt/projects --json
unisphere prep --target /absolute/prep --no-default-roots --root claude-code=/absolute/archive --modified-since 2026-01-01T00:00:00Z
unisphere prep compact --target /absolute/prep
unisphere prep record --target /absolute/prep --source claude-code/default/project/session.jsonl --offset 0 --include-content
unisphere prep recipes --human
unisphere prep recipe daily --target /absolute/prep | duckdb
```

Re-runs read only records after each source's committed cursor, stop at the last
complete LF (the remainder is a reported pending tail) and commit nothing when
nothing changed. A rotated, truncated or rewritten source, a changed snapshot
revision, fold policy, table schema or set root starts a new generation. Every
run reports, per set, discovered sources, counts by status (`new`, `unchanged`,
`appended`, `replaced`, `skipped`, `unreadable`, `unsupported`, `missing`) and
skipped symlinks, hidden and unreadable entries; JSON `data.sources` lists every
source that is neither unchanged nor skipped. Unreadable sources keep their
previous committed state and make the exit 3.

Output is `TARGET/state.json`, Parquet parts under `TARGET/tables/` and DuckDB view
definitions in `TARGET/views.sql` (`calls_v`, `turns_v`, `triggers_v`, `events_v`,
`compactions_v`, `tool_uses_v`, `sources_v`, `sessions_v`). Tables are
metadata-only unless `--include-content` adds `triggers.content_head`.
`prep record` emits one native record and is refused (exit 2) without
`--include-content`. Native stores are opened read-only. Envelopes use commands
`prep`, `prep.compact` and `prep.record`. `PrepCommand::explicit_sets`,
`wants_default_root` and `request` give embedding applications the parsed roots
and request; `run_prep` takes the merged `Vec<PrepSourceSet>` and an injected
`&dyn PrepApi` and holds no prep semantics. `unisphere docs get prep` is the
offline guide.

`prep recipes` lists the bundled DuckDB research recipes (envelope command
`prep.recipes`); `prep recipe NAME --target DIR` prints `SET file_search_path`,
`.read 'TARGET/views.sql'` and the named query to stdout in every output mode,
for `| duckdb`. Unisphere links and runs no engine. An unknown name exits 2
listing the valid names. `unisphere docs get research-recipes` maps each recipe
to its question.

## Git Notes attribution

```sh
unisphere sessions list --adapter git-ai --repo /absolute/repository
unisphere sessions export --adapter git-ai --repo /absolute/repository --git-executable /usr/bin/git
unisphere sessions export --adapter git-ai --repo /absolute/repository --notes-ref refs/notes/ai --commit FULL_COMMIT_OID --include-content
```

Git AI is an input format only: no installation, executable invocation, crate,
library, cache, daemon or HTTP service from Git AI is needed. The app resolves
standard `git` from PATH unless `--git-executable` names an absolute trusted Git
binary. Help works without Git. Missing Git returns `git_unavailable`, not a
Git AI installation error.

`--repo` accepts an explicit local normal/bare repository or linked worktree,
including a nested directory; relative paths resolve under the captured cwd.
The default ref is `refs/notes/ai`. Only full `refs/notes/*` refs are accepted.
Repeat `--commit` for full lowercase 40/64-hex commit IDs; duplicates do not
duplicate observations. Omitting it selects all notes in the one requested ref,
not all refs or repository history. Tracking refs are never auto-aggregated.

Listing writes one JSON envelope on stdout with `data.adapter` and `data.listing`.
The listing contains canonical repository/common-dir/git-dir/worktree identities,
normalized selection, requested ref, nullable pinned ref tip and note references
with target commit and blob IDs. A missing valid ref is a normal empty result,
not evidence that no AI work occurred.

Export writes one complete bounded OTLP LogsData JSONL batch on stdout and a
structural summary on stderr. `--output FILE` instead creates a new file outside
the canonical source worktree, per-worktree Git directory and common Git directory.
Existing files/symlinks and symlink-parent aliases into those roots are rejected.
Output parents must be caller-controlled; concurrent malicious parent replacement
is not a filesystem sandbox guarantee. Shell redirection and SDK-supplied writers
remain caller-owned.

| Flag | Default | Hard ceiling |
| --- | --- | --- |
| `--max-notes` | 1,000 | 10,000 |
| `--max-records` | 10,000 including manifest | 100,000 |
| `--max-note-bytes` | 1 MiB | 32 MiB |
| `--max-total-bytes` | 16 MiB | 64 MiB |
| `--max-listing-bytes` | 1 MiB | 64 MiB |
| `--command-timeout-ms` | 5,000 per command | 60,000 |

All limits are positive; total bytes must cover the per-note budget. Selected
fanout lookup avoids enumerating unrelated notes; `All` retains a bounded listing.
The existing writer independently enforces 32 MiB of encoded output. Input size
does not imply encoded size. Overflows, timeout, malformed/unsupported notes and
read errors never produce a successful truncated projection. Write/flush failures
may leave partial destination bytes; discard them before retrying.

Metadata retains native paths, keys, declared agent identities and supplied
statistics. Human-author strings, custom attributes and legacy messages/URLs are
omitted unless `--include-content`; content is emitted once per declared identity,
not copied into every line-range event. URLs remain inert. Missing/null counts
are not zeros, and checkpoint IDs are never OpenTelemetry span IDs.

Errors use `command: "sessions"` on stderr with `error.kind`, fixed `message` and
nullable `output_code` for shared writer errors. Invalid arguments exit 2;
operational failures exit 1. Important kinds include `git_unavailable`,
`unsafe_repository`, `unsupported_repository`, `unsupported_target`, `object_read`,
`invalid_ref`, `unsupported_format`, `invalid_data` and the named budget failures.
Partial clones/promisor configuration are refused before object reads; inherited
Git configuration/helpers are cleared, protocols disabled and ownership checks
preserved. Global/system `safe.directory` entries are deliberately not inherited:
use a caller-owned checkout. No fetch, push, note/config/index/hook mutation occurs.

Native note commands use the same typed root parser as queries and invoke an
app-owned constructor only after validation. Query `--source-adapter git-ai`
remains separate from native `--adapter git-ai`; neither reconstructs a transcript
or timing from attribution alone.
