# Plan 029 phase 2: session status for OMP, Copilot CLI and Codex

Requested by Jordan, relayed through pij-far-jackal on 2026-09-30. Today every non-Claude pij seat is `unknown`, so pij's cold-wake guard lets it through, and most of the fleet runs OMP.

## Outcome
`unisphere sessions status` and `StatusService::status_incremental` return the same `SessionStatus` for **Oh My Pi, then Copilot CLI, then Codex**, on macOS and Linux, with no Windows work.

The facts pij's guard needs, per harness:
- context used now
- context window (native where the harness records it, else `table@model-windows@1`, else `unknown`)
- last API call time
- cache lifetime where the harness records it

Everything else in the phase-1 shape comes along. A fact the harness does not record is reported as `unknown`, never 0.

## Why a new fold per harness
Plan 028 closed without its phase-2 folds. Status needs a pure `PrepFold` per dialect: native records go in, and out come call, turn and event rows plus `SessionFacts`. The existing query adapters in `crates/adapter-{omp,copilot-cli,codex}` already decode these formats, so each fold reuses that knowledge. The SDK status service, cursor and CLI stay as they are, because they are harness-neutral.

## Units (one Opus 5.5 OMP coder per harness, in parallel, delivered in priority order)
| unit | harness | owns | notes |
|---|---|---|---|
| tk-0101 | Oh My Pi (`oh-my-pi`) | `crates/adapter-omp/src/prep.rs` + tests/fixtures | first priority; per-message usage, model, compaction `tokensBefore` |
| tk-0102 | Copilot CLI (`copilot-cli`) | `crates/adapter-copilot-cli/src/prep.rs` + tests/fixtures | events JSONL; usage where recorded |
| tk-0103 | Codex (`codex`) | `crates/adapter-codex/src/prep.rs` + tests/fixtures | rollout `token_count` carries `model_context_window`, so the window is native |

PM work:
- **Contract delta** in `core::prep`, which 029 owns now that Plan 028 has closed: `SessionFacts.context_window: Option<i64>`, so a native window can reach status as `basis: native`.
- **Binding and window-table rows:** `crates/app/src/prep.rs` bindings, `model-windows@1` rows for the models these harnesses name (for example `github-copilot/claude-opus-5.5`), and `--pij` resolution for the omp, copilot and codex seat harnesses.
- **Proof:** per-harness fixtures through the external SDK consumer and the installed CLI. Linux runs in the ubuntu CI job and the `ubuntu:24.04` container script. Real-machine numbers cover live OMP seats.
- **Reviews:** a Sonnet 5.5 review of the phase-1 composition, running now, and then of each harness delivery.

## Delivery
As each harness lands:
1. harness checks and boot are green, and CI is green on ubuntu and macOS;
2. the branch `builder/029-session-status` is pushed, which Jordan has approved;
3. `pij send pij-yelling-trout "<rev> <harness id>"` goes out so pij maps it in `unisphere_harness()`;
4. far-jackal gets a one-line progress note.

## Current state
Phase 1 (Claude) is composed. It is live in pij #459 pinned to `0d48d80`. The Linux proof has landed and CI is running on `5465a25`. Next: the full verification plus the phase-1 composition review, running in parallel with phase-2 dispatch.
