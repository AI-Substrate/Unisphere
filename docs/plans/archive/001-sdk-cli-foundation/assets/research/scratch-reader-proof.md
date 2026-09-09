# Scratch native-reader experiment proof

**Status:** Verified experiments on the current macOS/Unix host; not shipping SDK adapters, approved common output or Plan001 acceptance.
**Date:** 2026-09-07.
**Location:** `scratch/native-readers/` in `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation`.

## Composition and ownership

A small shared `scratch-reader-core` contract/framing implementation was built first and passed 22 tests plus clippy. Separate agents then implemented `scratch-reader-claude`, `scratch-reader-omp` and `scratch-reader-copilot` concurrently without depending on the future product CLI or SDK. Main alone added workspace membership, formatted the union and ran all checks. A later targeted agent fixed the Copilot inspect error-rendering leak found by a real smoke test; an independent boundary reviewer reported no additional material findings.

Every reader accepts an explicit file path and caller-owned cursor/limits, extracts optional native identifiers/kind/role/time, and retains the parsed raw JSON record. This is an observation seam, not the final canonical event format. No real user session stores, registry files, network ingestion or global configuration were used.

## Observed verification

| Check | Result |
|---|---|
| `cargo test --manifest-path scratch/native-readers/Cargo.toml --offline --workspace` | 68 passed after the diagnostic correction |
| `cargo clippy --manifest-path scratch/native-readers/Cargo.toml --offline --workspace --all-targets -- -D warnings` | passed |
| `cargo fmt --manifest-path scratch/native-readers/Cargo.toml --all --check` | passed |
| `git check-ignore scratch/native-readers/core/src/lib.rs scratch/native-readers/Cargo.lock` | both excluded |
| Real invalid-UTF8 input through each inspect executable | exit 1, empty stdout, no source byte array or sensitive marker in stderr |

Successful smoke uses `cargo run --manifest-path scratch/native-readers/Cargo.toml --offline -p <package> --example inspect -- <explicit fixture>` from the worktree root:

| Package / owned fixture | Records | Cursor byte offset | Partial tail | Resets |
|---|---:|---:|---|---|
| scratch-reader-claude / `claude/tests/fixtures/b1d6f4fb-bd8e-4a10-a018-4205f4058b8e.jsonl` | 35 | 41843 | false | none |
| scratch-reader-omp / `omp/tests/fixtures/2026-08-26T07-46-01-430Z_01a03d08-7c56-7000-ac9b-95c4b3ef34d7.jsonl` | 193 | 323908 | false | none |
| scratch-reader-copilot / `copilot/tests/fixtures/copilot.events.jsonl` | 28 | 49233 | false | none |

The explicit fixture argument includes the `scratch/native-readers/` prefix when invoking from the worktree root. Examples emit aggregate counts/kinds/cursors only, not native content.

## Fixture provenance

Fixtures originate from Flowspace3's committed sanitized `crates/testkit/fixtures/conversations/` corpus. Main verified copied/projected fixture SHA-256 values after a shared-eval variable collision was corrected:

- Claude: `30d827a3b52d0b65885862c78afec0673065775248d648e2c64d81b183ce4469` (byte-identical copy).
- omp: `014eb80691eb3b306b5d1c3f638f0fb8c576bc4956ab16a6133da5e7b6ea799f` (byte-identical copy).
- Copilot native JSONL: `56aae42b294ef897481ddd43d2655a480beb6ac40a2ca63e53398f121dca29de`; projected from sanitized metrics fixture SHA `0de99805d337eba6e3d844651de791a046a5cf968d1378b670d0845899c1a954`, selecting only github-copilot-cli/event_kind=5 rows ordered by id and emitting native v.0 values. The owned extraction script records this process.

## Declared limits and promotion conditions

- Parsed JSON values are preserved, not original whitespace/key order/duplicate-key byte fidelity.
- Readers do not discover stores, group Claude blocks, merge sidecars, resolve artifact spills or produce a normalized telemetry/session model.
- omp append cursors do not reread an earlier in-place title change; native header/session fields are not silently cached across calls.
- Unix device/inode plus size detects the exercised rotation/shrink cases, not inode reuse, arbitrary in-place rewrites or truncate-and-regrow between observations. Non-Unix resumption is explicitly refused.
- Pij identity/roles are not fabricated by these readers; the workshop owns the proposed enrichment boundary.
- The Copilot fixture is a sanitized collector-harvested selection, not a complete native session; synthetic boundary tests cover fields absent from that sample.
- Scratch is gitignored and no shipping crate imports it. Retain the worktree until useful experiments are deliberately promoted or preserved; a Git commit does not back up ignored source. Promotion needs accepted scope, a reviewed common contract and applicable integration proof, not a blind file move.

These results prove the experiment's declared explicit-file reading behavior. They do not make the foundation product implemented, the output workshop approved or the deferred Flowspace3 integration complete.
