# Plan 029 composition review r2

Reviewer `pij-easy-seahorse` (omp, github-copilot/claude-sonnet-5.5). Subject: commit `0a5d16b9370c81b0adf5ea08d7bff281755ad38e` (packet, docs only); verified code is its parent `a57491d90ce6038962b6c68d8a315390e6130697`. Supersedes `rv-029-composition-r1`. Read-only.

## Verdict: approved

The composed session-status feature is fit to land at `a57491d`. The PR to main remains the operator's separate decision.

## r1 findings

- **F-01 fixed (confirmed by probe).** `is_plain_component` gates the top of `StatusService::status_incremental`, before bindings or any read; it refuses empty, `.`, `..`, and ids containing `/`, `\` or NUL. Rebuilt `unisphere` at `a57491d` and re-ran the r1 exploit plus variants with `HOME=/tmp/rv/home`: `../../../x/z`, `../x/z`, `a\b`, `.`, `proj/../outside`, `/tmp/rv/x/z` all return `UNI-STATUS-TRANSCRIPT-NOT-FOUND` "the session id must be a single path component" (r1 read `/tmp/rv/x/z.jsonl`). The gate is in the SDK, so `--pij`, pane resolution and embedders are covered. Positive control: a plain id `y` under `~/.claude/projects/p/y.jsonl` still returns `ok: true`. Test `a_session_id_that_is_not_one_path_component_is_refused_before_any_read` passes with the sdk suite (23 passed); cli status suite 23 passed.
- **F-02 fixed.** `crates/cli/docs/session-status.md` window row now says the table assumes 1M, `percent` can understate a 200k session, and GPT windows are left unknown.
- **F-03 accepted.** Reason (Claude writes `procStart` in UTC, `ps` reports local time; pane-id match plus a live descendant pid narrow the stale-record window) is sound and recorded in the plan. Known limit, not a landing blocker.
- **F-04 accepted.** Guide checks were run directly at the reviewed SHA and this is to be stated in the PR description; CI and `harness checks --json` were green at r1. Acceptable.

## Delta scope
`git diff ef47ccc..a57491d` touches only `crates/sdk/src/status.rs` (+13), `crates/sdk/tests/status.rs` (+28), the CLI doc row, refreshed real-proof evidence, plan text and review files. No new dependencies or behaviour beyond the gate.

## Gaps
Did not re-run `harness checks --json`/`boot`, CI, Linux container proof or the real-machine script at `a57491d`; relied on the sdk and cli status tests and the probes above. No provider-side model attestation beyond the harness-reported model string.
