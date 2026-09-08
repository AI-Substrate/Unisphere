# Plan 005 — corrected-baseline conformance check (baseline r2)

**Verdict: approved.** B1 is closed by the real gate rather than by argument: `cargo run -p
unisphere-testkit --bin unisphere-arch-check` now exits 0 against actual `cargo metadata`, printing
`architecture: 16 declared normal/dev/build edges accepted`. B2–B5 all landed, and B2 landed as the
*correct* fix rather than the one I originally proposed. Three new low findings follow; none blocks
the source-bound seal, and I would not hold the three coder lanes for any of them.

| Field | Value |
| --- | --- |
| subject_sha | `ac4d28f682739c36f08bb5b8f3dd1c605a7e1a86` ("fix(collection): align baseline sensors and prove injected integration seams") |
| parent | `3df2450324873eedb77aba645edfaa9472de3a56` (the r1 subject) |
| manifest | `assets/baseline-corrected-manifest.json`, 8 files, all digests recomputed and matching |
| proof | `assets/baseline-corrected-proof.json`, 5 checks, all exit 0 |
| plan / guide | unchanged and approved: `0c28b557…`, `5520d593…` |
| scope | decomposition — frozen baseline vs approved contracts; SDK/CLI integration explicitly excluded |
| method | committed bytes only; no builds, tests, lint or formatters run by me |

The corrected manifest deliberately carries no `source_sha`, on the stated grounds that source and
proof are committed together so the future commit id cannot be written into the file it is committed
with. That reasoning is sound, and it leaves the bytes-to-commit binding to the reviewer, so I made it
explicitly: for each of the 8 paths `git diff --quiet ac4d28f6 -- <path>` reports SAME, and each
working-tree digest recomputes to the manifest value. The bytes I read are the bytes at `ac4d28f6`.

**Seven of the eight frozen files are byte-identical to the r1 subject** — `Cargo.toml`,
`crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/collection.rs`,
`crates/testkit/src/lib.rs` and both fixtures all produce an empty diff against `3df24503`. Core was
not touched at all. So the C1/C3/C4/C5 conformance I verified line by line in r1 carries forward
unchanged and is not re-audited here. Only `crates/testkit/src/collection.rs` changed
(`8ce28bb3…` → `6b63d50f…`), and its three changes are exactly B2, B3 and B4.

My r1 report is committed at `assets/reviews/baseline-opus-r1.md` with sha256
`3df410b27f4358a78f638090cda4e90938c5ffa66fd5a8042104445d4b37aee8` — byte-identical to what I wrote,
so the r1 record and its ingestion refusal stand unaltered as history.

---

## Disposition of r1 findings

### B1 — closed, with real evidence

`allowed()` now reads `("unisphere-core", "normal") => matches!(dependency, "serde" | "serde_json")`,
`fixtures/architecture/allowed.json` gained the `serde_json` edge, and the three future packages were
added both to the member allowlist in `check()` and to `allowed()` with exactly the C12 edges —
`unisphere-loader-jsonl -> core | libc`, `unisphere-adapter-claude -> core | serde_json | time`,
`unisphere-output-otlp -> core | serde_json`, and `unisphere-app` extended to depend on all three.
That is a faithful transcription of C12, including the target-unix libc edge and the parsing-only
`time` edge, so the wave-1 lanes will not hit the member-name rejection I flagged.

What closes the finding is that the proof no longer stops at scoped `--lib` tests. It records the
architecture binary actually running against real metadata at exit 0 with a printed edge count, which
is the check `checks.mjs:85` wires, and it adds a full-workspace `cargo clippy --workspace
--all-targets -- -D warnings` at exit 0. The repository gate is green at the commit proposed for seal.

Two observations, neither a finding. The proof's arch-check invocation omits `--locked`, which
`checks.mjs` passes; since the working tree is clean at `ac4d28f6` apart from an untracked
`.dd/schemas/builder/work-packet/`, `Cargo.lock` cannot have drifted during those runs, so the
`--locked` form would behave identically — inferred from cleanliness, not executed. And the 16-edge
figure is the PM's recorded runtime output, not something I reproduced.

### B2 — closed, and closed correctly

The frozen helper now documents the split in the artifact a later reader actually opens:

> Output bounds here mean one mapped record per supplied physical record. The OTLP writer separately
> enforces MAX_OUTPUT_BATCH_BYTES while encoding: serializing these Rust DTOs would not measure the
> OTLP representation.

That is the auditable interpretation I asked for, sitting directly above `assert_adapter_conformance`
where the apparent gap against C13's `outputbounds` wording would otherwise be read as an omission.

The shared sensitive-marker detector is better than a comment: `serde_json::to_string(&metadata)`
followed by `assert!(!metadata_json.contains("SENSITIVE-"))`. Crucially the change also rewrote the
test input from `b"sensitive text\n"` to `b"SENSITIVE-TEXT\n"`, so the detector is now exercised
against a payload that would trip it — and the same test still asserts that the marker *does* appear
in `body` under `include_content: true`. The sensor is therefore proven to discriminate rather than
proven only to pass, which is the difference between a test and a decoration.

### B3 — closed

`fixture_records` now carries: "Blank means every physical byte is ASCII whitespace (including space,
tab, CR and LF); blanks consume offsets and the loader's physical record budget." That pins both
halves — the predicate and the C3 budget clause. The dispositions record commits to repeating it in
the loader supplement, which is the surface the `tk-0002` author will actually read.

### B4 — closed

`let collector: &dyn CollectionApi = &fake;`, with both the failure and success assertions dispatched
through the trait object. Object safety is now compile-proven at wave 0 rather than asserted in prose.

### B5 — closed, and expanded well beyond what I asked

I asked for two false positives to be avoided; the response was a tested purity sensor. `check_source`
strips a trailing `#[cfg(test)]` module and `//` comment lines before scanning, and
`fixtures/architecture/purity.json` supplies eleven cases driven by
`purity_sensor_rejects_negative_sources_but_permits_core_ports` — eight negatives covering
fully-qualified `std::fs`, grouped `use std::{collections::BTreeMap, fs}`, `env`, `process`, `net`,
`SystemTime::now`, `include_bytes!` and an `unsafe` block, and three positives covering exactly the
traps I named: `#![forbid(unsafe_code)]` beside a `std::io::Write` core port, a trailing
`#[cfg(test)] mod tests { use std::fs; }`, and a doc comment that merely *mentions* `std::fs`. The
`unsafe` check tokenises on alphanumeric-plus-underscore, so `unsafe_code` is a single token and the
`forbid` attribute cannot match. `OffsetDateTime::now_utc` is also denied, which anticipates the one
way `tk-0003`'s approved `time` dependency could become a clock read. Symlinked entries are refused
rather than followed.

The scan covers `crates/core/src` and `crates/adapter-claude/src`, matching C13's scope; the printed
`5 core/adapter production source files` is the complete core production set (`lib.rs`,
`collection.rs`, `config.rs`, `errors.rs`, `ports.rs`) plus zero for the absent adapter. The header
comment is honest about what it is — "a bounded lexical sensor, not a Rust effect system" — and the
printed line repeats that independent review is still required. Block comments and string literals
remain outside the comment filter, which is inherent to a lexical scan and correctly disclaimed
rather than oversold.

---

## New findings

### C1 — low, open — the fixture edge count was removed rather than updated

`accepts_only_declared_allowed_edges_in_a_partial_workspace` changed from
`assert_eq!(check(fixture).unwrap(), 7)` to `assert!(check(fixture).is_ok())`. The dispositions record
this deliberately as "incidental numeric-edge test pin removed", so it is a disclosed decision and not
an oversight — but it is worth naming what the sensor lost. The count was the only assertion that the
fixture still *exercises* the edges it claims to; `is_ok()` is satisfied by a fixture that has quietly
stopped covering anything, since `check` returns `Ok` for an empty package list too. Neither layer now
pins a count: the real gate prints 16 but returns `Ok` on any number. Rejection is still well covered
by `forbidden_graph_fixtures_are_rejected_with_the_edge_kind` and
`malformed_or_incomplete_graphs_never_pass`, which is why this is low rather than material. Restoring
`assert_eq!(…, 8)` costs one line per future edge and buys back detection of a vacuous fixture.

### C2 — low, open — the purity scan has a silent-zero path

`check_sources` returns `Ok(0)` for a missing directory, which is right for `adapter-claude` before
wave 1. But the paths are CWD-relative (`crates/core/src`), so an invocation from any other working
directory prints `purity: 0 core/adapter production source files checked` and exits 0 — a purity
sensor reporting success having examined nothing. This cannot fire through `checks.mjs`, which runs
`cargo run` from the repository root, so exposure today is theoretical. It is also one line to close:
require the core scan specifically to return a non-zero count, leaving the absent-adapter zero
tolerated. That also protects against a future layout move silently disabling the check, which is the
same drift class B1 was.

### C3 — low, open — the dev-dependency arm was widened for six packages to serve one

The dev arm went from `dependency == "unisphere-testkit"` to
`matches!(dependency, "unisphere-testkit" | "tempfile" | "serde_json")`, applied to `unisphere-sdk`,
`unisphere-cli`, `unisphere-app` and all three future crates. The actual need is narrow and
legitimate: `crates/cli/Cargo.toml` adds `tempfile.workspace = true` under `[dev-dependencies]` for
the new CLI tests, and `tempfile` is already in-graph via testkit. C12 enumerates only "each new crate
devdepends testkit", so the grant now exceeds the contract's enumeration for five packages that have
not asked for it — including three crates that do not exist yet, whose lanes inherit a pre-approved
dev surface nobody reviewed. An allowlist should be as tight as the approved need; granting `tempfile`
to `unisphere-cli` and leaving the rest at `unisphere-testkit` keeps the sensor honest. Dev-only, so
low.

Worth stating plainly because it is a boundary question rather than a bug: this widening was made in
the same commit as, and to accommodate, the SDK/CLI integration work I was told not to review. I have
not reviewed that code and this report makes no claim about it. I am only noting that a sensor inside
the frozen fence was loosened to fit it.

---

## Scope and honesty

Committed bytes only, at `ac4d28f6`. I ran no build, tests, clippy, rustdoc or formatters, so every
runtime number here — 20 baseline tests (6 core + 14 testkit), 4 checker tests, 16 edges, 5 purity
sources, 0+18+7 CLI tests, clean full-workspace clippy — is the PM's recorded evidence read out of
`baseline-corrected-proof.json`, not reproduced by me. The proof's `future_package_directories_absent`
list is corroborated independently: `crates/` contains only `app`, `cli`, `core`, `sdk` and `testkit`
in both `git ls-tree` and the working tree.

`crates/sdk/src/collection.rs`, `crates/cli/src/sessions.rs` and their tests are committed at this SHA
and remain outside review scope; I did not read them and nothing here speaks to C10 or C11. C2, C6,
C7 and C8 still have no implementation to check. No design contract was reopened, no mapping-profile
question revisited, and the r1 artifacts were left untouched.

Expected next step — source-bound seal, then the three coder lanes — is supported by this evidence.
