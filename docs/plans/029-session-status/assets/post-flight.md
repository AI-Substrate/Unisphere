# Plan 029 post-flight: session status

**Landed:** PR #10 was merged to `main` as `0370cabbe4d9bcbb5b7bf6b0e13a490f0f4d0d59` (2026-09-30, merged by Jordan through pij-far-jackal). The branch head `78c2eed` is in `main`.

**Tested and reviewed artifact:**
- The code was reviewed at `0a5d16b` (composition r2 approved, `rv-029-composition-r2`).
- Two later commits are CI or merge only:
  - a merge of `origin/main` (Dependabot config and `SECURITY.md`);
  - a CI job timeout raised from 30 to 45 minutes.
- CI is green on ubuntu-latest and macos-latest at `78c2eed`.

## Delivered
- `unisphere sessions status`, plus the SDK calls `StatusService::status` and `status_incremental`.
  - It returns one harness-neutral `SessionStatus` (schema v1), where every fact names its basis or is listed as unknown.
  - Lookup works by `--pij`, `--pane` or `--session` + `--harness`.
- **Harnesses:**
  - Claude Code (phase 1).
  - Oh My Pi, Codex and Copilot CLI (phase 2), on top of Plan 028's approved folds. Plan 028 is absorbed and landed by this PR.
- **Platforms:** macOS and Linux. Windows was requested and then dropped by Jordan; existing Windows behaviour is unchanged.
- **Live consumer:** pij Plan 157 embeds the SDK as its cold-wake guard and is repinned to `main` `0370cab`.

## Proof pointers
| AC | Evidence |
|---|---|
| ac-0001 lookup | `crates/loader-query/tests/status_target.rs`; `assets/proof/status-real-evidence.json` (`native_pane`, `largest_seat_by_pij`); `assets/proof/linux/linux-evidence.json` |
| ac-0002 one shape | `crates/core/tests/status_contract.rs`; `unisphere-proof status` (in `harness boot`) |
| ac-0003 model / context | `crates/sdk/tests/status.rs`; `status-real-evidence.json` (`model_switch`) |
| ac-0004 activity facts | `crates/sdk/tests/status.rs`; adapter `tests/prep.rs` (claude, omp, codex, copilot-cli) |
| ac-0005 big and live | `status-real-evidence.json` (`largest_by_session`, `live_append`) |
| ac-0006 SDK/CLI parity, cursor, no sqlite | `unisphere-proof status`; `fifty_seats` (warm 0.16–11 ms); arch-check; Pij dogfood findings (Plan 157 evidence) |
| ac-0007 harnesses and docs | adapter fold tests; `crates/cli/docs/session-status.md`; `docs/cli.md`; `docs/sdk.md` |

Reviews (all in `assets/team/`):
- decomposition r2: approved;
- composition r1: changes requested; F-01 path traversal and F-02 documentation fixed;
- composition r2: approved.

## Accepted limits, deferred items and human rulings
- **Pane lookup** takes about 50 ms, bounded by one `ps` call (Jordan's ruling). The measured median was 52 ms on macOS and 8.8 ms on Linux.
- **Copilot CLI context used is unknown:** Copilot saves no per-call usage. A possible follow-up is to report the latest shutdown or compaction `currentTokens` with a derived basis.
- **GPT model windows are unknown,** because they differ by provider. Claude's 200k vs 1M variants can't be told apart, so the table assumes 1M (documented).
- **F-03:** the native Claude pane record is not checked against procStart (accepted).
- **F-04:** Builder `compose --verify` refused after phase 2 was added post-seal. The guide's checks were run directly instead, and the reviewer accepted this.
- **Not exercised on Linux:** real Claude Code; the Linux records were synthetic.

## Highest-leverage encodable improvement
A checks test asserting that the ok envelope stays under 32 KiB. A growing test log broke CI parsing silently (retro DL-006).
