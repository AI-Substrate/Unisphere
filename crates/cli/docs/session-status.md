# Session status

## Motivating question

What model is this agent on right now, how full is its context, how long has it been idle, and is its prompt cache still warm, without knowing the harness's file format?

## Ask

`unisphere sessions status` takes one or more queries and returns one result per query, in argv order:

- `--pij ID`: a Pij seat. Unisphere reads `pij list --json`; the seat's recorded process `(pid, proc_start)` must still be running.
- `--pane %N`: a tmux pane. Unisphere uses every live Pij seat recorded on that pane and a native lookup: the tmux pane's process, a bounded walk of its child processes (depth 4, 64 processes) and the harness's own session record (Claude Code: `~/.claude/sessions/<pid>.json` with `sessionId` and `tmux` pane). The native lookup needs no Pij daemon.
- `--session ID --harness HARNESS`: an explicit native session. `HARNESS` is an adapter id such as `claude-code`. One `--harness` applies to every `--session`; otherwise give one `--harness` per `--session`, in the same order.

Repeat and mix them in one call. Output is JSON when stdout is not a terminal and a table on a terminal; `--json` or `--human` chooses explicitly.

Supported harnesses: Claude Code. Any other harness returns the `UNI-STATUS-UNSUPPORTED-HARNESS` result for that query; the other queries still answer.

## Resolution

Every result carries `status.resolved`:

| Field | Meaning |
|---|---|
| `query` | The query as given: `{"pij": ID}`, `{"pane": "%N"}` or `{"target": {...}}` |
| `target` | `harness`, `session_id` and `transcript`: the transcript file actually read |
| `pij_id` | The Pij seat that answered, when one did |
| `pane` | The tmux pane, for pane queries and seats that record one |
| `basis` | `explicit` (`--session`), `pij_registry` (live Pij seat) or `native_pane` (harness record under the pane) |
| `conflicts` | Every other distinct answer, each with its `basis` and `target`. A disagreement is returned, never silently picked |

For a pane, live Pij seats come first (most recent event first), then native records (shallowest process first); the first is the target and the rest are conflicts. Pij unavailable is not a pane failure: the native lookup still answers.

## Facts

Every fact is a `{value, basis}` pair or bare value, or it is absent and its name is listed in `unknown`. A fact the harness does not record is never reported as 0.

| Basis | Meaning |
|---|---|
| `native` | Recorded by the harness |
| `derived` | Computed from native facts and the current time |
| `table` | Looked up in a versioned table named by `context.window_table` (`model-windows@1`) |
| `mtime_fallback` | Transcript modification time, only when no native timestamp exists |

| Fact | Definition |
|---|---|
| `model.current` | Model of the latest main-chain, non-synthetic call |
| `model.pending_switch` | A `/model` switch recorded after that call; the next call uses it |
| `model.history` | Main-chain model spans in order: model, first/last call time, calls |
| `context.used_tokens` | Latest main-chain call's input + cache read + cache write (native) |
| `context.window_tokens` | Native when recorded, else `model-windows@1` table (basis `table`), else unknown. Claude Code transcripts do not say whether a model ran with a 200k or 1M window; the table assumes the larger one, so `percent` can understate how full a 200k session is. Windows for GPT models differ by provider and are left unknown. |
| `context.percent`, `context.display` | Only when used and window are both known, e.g. `250k of 1M (25%)` |
| `last_call` | Time, input, output, cache read, 1h and 5m cache writes, stop reason when recorded |
| `last_call.ttl_bucket` | `1h` or `5m`, from where the last call wrote cache (derived) |
| `last_call.cache_warm` | Now minus last call time is under the TTL (derived) |
| `timeline.created_ms` | First native event |
| `timeline.last_updated_ms` | Latest native event, else transcript mtime (`mtime_fallback`) |
| `timeline.idle_seconds` | Now minus last updated |
| `turns` | Total and last-hour turns, each also by origin (`human`, `peer`, `task-notification`, ...) |
| `compaction.counts`, `compaction.last` | Manual/auto compactions and the last one's time, trigger, pre/post tokens and first context after it. Claude records markers, so no markers is 0 |
| `calls` | API calls: total and sidechain |
| `limits_seen` | Usage-limit notices the transcript recorded, with the native reset phrase |
| `source` | Bytes read, pending partial tail bytes, and why a cursor was reset |
| `unknown` | Names of facts this harness or session cannot supply |

Timestamps are epoch milliseconds. `schema_version` is 1. Output is metadata only: no transcript content.

## Output and exit

JSON: `{"ok": true, "command": "sessions.status", "v": 1, "data": {"results": [...], "failed": N}}`. Each result is `{"ok": true, "query", "status"}` or `{"ok": false, "query", "resolved", "error": {"kind", "code", "message", "fix"}}`; `resolved` is present when resolution succeeded but the status read failed.

Human: one table row per query (model, context, idle, turns with last hour, compactions, cache state), then conflicts and failures with their fix.

Exit: 0 every query answered, 3 at least one query failed, 2 invalid arguments, 1 output could not be written.

| Code | Meaning |
|---|---|
| `UNI-STATUS-PIJ-UNKNOWN-SEAT` | Pij lists no seat with that id |
| `UNI-STATUS-PIJ-NO-SESSION` | The seat has no recorded native session |
| `UNI-STATUS-DEAD-BINDING` | The seat's recorded process is no longer running |
| `UNI-STATUS-PANE-NOT-FOUND` | tmux lists no such pane |
| `UNI-STATUS-UNSUPPORTED-HARNESS` | The harness has no status support, or no supported harness runs in the pane |
| `UNI-STATUS-PIJ-UNAVAILABLE` | `pij list` could not answer a `--pij` query |
| `UNI-STATUS-TRANSCRIPT-NOT-FOUND` | No transcript for the session under the configured roots |
| `UNI-STATUS-READ` | The transcript could not be read |

## Examples

Every `unisphere` line below is parsed by the test suite.

```sh
unisphere sessions status --pij pij-able-stoat
unisphere sessions status --pane %3 --human
unisphere sessions status --session b9cf6f3c-2a9f-4f14-a012-80cba68f831e --harness claude-code --json
unisphere sessions status --pij pij-able-stoat --pane %3 --pane %11
unisphere sessions status --harness claude-code --session b9cf6f3c-2a9f-4f14-a012-80cba68f831e --session dd54bc01-c29b-43b2-8780-f376060424d3
```

## SDK reuse

The status facts come from `unisphere_sdk::status::StatusService`, which implements `unisphere_core::status::SessionStatusApi` for an explicit `StatusTarget` and never calls Pij, tmux or processes. Its `status_incremental` op keeps an opaque cursor so a re-status reads only appended bytes. Pij and pane lookup is the CLI's `unisphere_loader_query::status_target::StatusTargetResolver`.

**Next step:** run `unisphere sessions status --pane %N --human` for the pane you are in.
