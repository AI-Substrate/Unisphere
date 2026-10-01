# Plan 029 composition review r1 (phases 1 + 2)

Reviewer `pij-easy-seahorse` (omp, github-copilot/claude-sonnet-5.5). Subject: commit adding the packet, `21409164430ff7fdda06a2068ae6b9d0ad1a7a6e` (docs only); verified code is its parent `ef47ccce189b2542f56bc92ba15e11ef99e5f98f`. Read-only; no fixes made.

## Verdict: changes-requested

One medium finding (F-01) must be fixed before landing; it is a few lines plus one test. Everything else is sound or an acceptable follow-up.

## Findings

### F-01 (medium, fix before landing) — session id is joined into a path unchecked; probes escape the configured root
`crates/sdk/src/status.rs` `StatusService::locate`, no-transcript branch: `format!("{}/events.jsonl" | "{}.jsonl", target.session_id)` is matched against the fold glob (`**/*.jsonl` for Claude) and then `set.root.join(relative)` is stat'd and read. A session id containing `..` passes the glob and leaves the root.

Reproduced with the built CLI at `ef47ccc`: `HOME=/tmp/rv/home unisphere sessions status --harness claude-code --session '../../../x/z'` read `/tmp/rv/x/z.jsonl` (outside `~/.claude/projects`) and returned a full status, `target.transcript` = that path. `--session 'a/b'` and `..` did not resolve only because no such file existed.
Why it matters: `--pij` and native-pane paths, and embedders (pij Plan 157), take the id from registry/record data, not from the operator. Also violates the guide's own rule (explicit path under a root, else parent-as-root; ids only discovered by stem). Loader `stat` refuses symlinks, which limits but does not close this.
Fix: reject any `session_id` that is not one path component (empty, `.`/`..`, contains `/`, `\`, NUL) in `StatusService` (SDK boundary, so every embedder is covered) with a typed failure; add a contract test with `../` and `/abs` ids; optionally reject at CLI arg parsing too.

### F-02 (low, follow-up) — table window 1M for Claude Opus/Fable is a family assumption
`MODEL_WINDOWS` maps `claude-opus-4-8`/`claude-opus-5`/`claude-fable-5` to 1M. Claude Code transcripts carry the same model id for 200k and 1M sessions, so `context.percent` is understated for 200k users. Basis is honestly `table` and named (`model-windows@1`), but `crates/cli/docs/session-status.md` does not say the table cannot see the plan/variant. Add that sentence; consider a later `model-windows@2` that treats a window as native only when observed.

### F-03 (low, follow-up) — native Claude pane record is not checked against process start
`claude_record` accepts `~/.claude/sessions/<pid>.json` for any live pane descendant with that pid; `procStart` is deliberately not compared (UTC vs local). A stale record whose pid was recycled inside the same pane could name a dead session. Pane id in the record is checked, which makes this narrow. Acceptable; record as known limit.

### F-04 (low, process) — `compose --verify` E475 workaround
Running guide checks directly is acceptable for landing because CI is green on both OSes at `ef47ccc`, and `harness checks --json` re-run here returned `status: ok` (exit 0). Not independently re-run by me: `harness boot`, CI, and each guide check id. Before the PR, re-seal or amend the guide (phase 2 added after seal) so the composed proof is machine-verifiable rather than by narrative.

## What I verified
- Contract/fact definitions (SDK `derive`): unknown facts stay in `unknown` (never 0); model/pending switch, post-compaction context, native window wins over table, TTL only from a native split or native lifetime, mtime fallback labelled `mtime_fallback`. Smoke with a synthetic Claude record with no cache fields gives `context.used_tokens` null (honest); with full usage gives `115 of 1M`, `ttl_bucket 1h` derived, `cache_warm false` at 5513 s idle.
- Real OMP spot-check on this seat: `unisphere sessions status --pij pij-easy-seahorse --json` resolved via `pij_registry`, 707 KiB read, 56 calls, 6 peer turns (matches the 6 peer messages received), `unknown: []`, compaction trigger null with `unknown_trigger: 4` (honest), cache 5m warm.
- Cursor semantics (code read + tests): single-use fold, clones share one `LiveFold`, unchanged stat returns without consuming it, resets named `target-changed | rotated | policy | truncated | anchor | spent`; error keeps caller's previous cursor. Snapshot sources refold whole.
- Resolver: no shell, `LC_ALL=C`, 5 s timeout, 4 MiB output cap, bounded process walk (depth 4, 64 processes), 64 KiB record cap, liveness by `(pid, proc_start)`, dead/no-session/unsupported kept distinct, distinct pane answers returned as conflicts. Linux and real-machine evidence files show `failures: []` (mac: 50 seats 1.65 GB cold 5.3 s, warm <12 ms, pane median 51.9 ms; Linux ubuntu container incl. recycled-start dead binding).
- Runs: `cargo test --locked` for sdk `status` (7), loader-query `status_target` (9), cli `status` (22) all passed; `harness checks --json` ok.

## Landing judgment
Fix F-01, then landable. F-02..F-04 are acceptable follow-ups. Copilot context-used unknown and GPT windows unknown are acceptable: both surface as `unknown` with no invented value.

## Gaps
Did not run `harness boot`, CI, Linux container, or the real-machine script; relied on the evidence files and the exercised checks above. Phase-2 folds for Codex/Pi/Copilot/VS Code/Cursor were not spot-checked against real sessions. No provider-side model attestation beyond the harness-reported model string.
