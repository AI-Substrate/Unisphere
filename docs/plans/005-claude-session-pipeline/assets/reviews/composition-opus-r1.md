# Plan005 composition review — r1

- **Subject:** `093a89811296efb681386047b1da912348c63f10` — `feat(collection): compose explicit Claude SDK CLI pipeline and document fidelity gaps`
- **Scope:** composition (whole agreed Claude pipeline), plus judgement of `docs/fidelity.md` as a delivery **report**, not a new implementation gate
- **Reviewer:** `pij-huge-nigel` (omp, github-copilot/claude-opus-5, effort high), independent of every implementing peer
- **Verdict:** **approved** — no unresolved material finding; four advisories recorded with `accepted` dispositions
- **Method:** read-only inspection of committed bytes. No build, test, clippy, rustdoc, formatter, or canonical edit was run by me. Runtime results are read as **recorded evidence**.

## Binding

| Artifact | Digest |
| --- | --- |
| `docs/plans/005-claude-session-pipeline/plan.dd.json` | `0c28b557a2def3dc075c70403e6768959057c007c428dcb9a4ac23683ac7f830` (matches packet) |
| `docs/plans/005-claude-session-pipeline/assets/impl-guide.dd.json` | `5520d59344dc2f664af3121df3184c344f659a4c1acceca9432ce06abe242777` (matches packet) |
| Baseline fence `ac4d28f6…` → `093a8981…` | all 8 frozen files recompute to `baseline-corrected-manifest.json`; `git diff --stat` over those paths is empty |
| Compose-verify receipt `assets/team/composition.dd.json` | `fef0a9a5d8be8a2e66f8e749279156dc5a7726ba3dc319bb13d181da0553b6ed`, `artifact_sha` = subject |

Diff against the reviewed baseline is 89 files / +9388 / −80, adding `crates/loader-jsonl`, `crates/adapter-claude`, `crates/output-otlp`, `crates/app/src/adapters.rs`, `crates/sdk/src/collection.rs`, `crates/cli/src/sessions.rs`, the collection proof mode, six docs and the plan evidence set.

## Folded compose-verify result

`assets/composition-verification-summary.json` (`b2199d63b35a418e5769c3920f59f6c79c4dd587077e32e0af43c5189525e8b4`) records first-class verify at this exact candidate: `vd-0005` workspace tests, `vd-0006` architecture, `vd-0007` clippy `-D warnings`, `vd-0008` fmt, `vd-0009` doc tests, `vd-000a` `unisphere-proof collection`, `vd-000b` `harness checks --json`, `vd-000c` `harness boot --json` — all `exit_code 0`. Ten warning-first ownership notes are preserved, no waiver. I did not re-run any of it and I do not restate it as my own execution.

**Recorded, not committed:** at review time `composition.dd.json` and `composition.dd.md` are modified and `composition-verification-summary.json` / `observation-custody.json` are untracked. Source `HEAD` is unchanged, so the verify binds to the reviewed bytes — but the receipt must be committed before it can be read at a bound SHA.

## Source verdicts

**Purity and side-effect ownership — holds under inspection, not just under the sensor.**
`crates/adapter-claude/src/lib.rs` imports exactly `std::collections::BTreeMap`, `serde_json` and `time`; the only `time` use is `OffsetDateTime::parse(_, &Rfc3339)`, never `now_utc`. No fs, env, process, net, clock or destination reaches the mapper — it receives `&[NativeRecord]` and returns `MappedBatch`. `crates/output-otlp/src/lib.rs` touches only `std::io` and writes solely to the caller-supplied `&mut dyn Write`. `crates/sdk/src/collection.rs` has no I/O of its own. The only `OpenOptions::open` in the product path is `crates/cli/src/sessions.rs:174`, `create_new(true)`, in the explicit CLI shell. All four new crates carry `#![forbid(unsafe_code)]`.

**Loader (`ac-0002`, `ac-0003`) — the hard parts are right.**
Listing is non-recursive, extension-filtered, sorted, and rejects a symlinked leaf via `symlink_metadata`; `read_dir` file-type checks drop symlinked candidates without opening them. Reads use `O_NOFOLLOW | O_NONBLOCK`, then re-check `metadata.is_file()`, so a FIFO leaf is refused rather than blocking on a writer. Resume validates that byte `offset-1` is LF, so a mid-record cursor is `SourceChanged` rather than silent corruption. `file.take(observed_end - offset)` freezes the boundary, which is what makes "EOF is an observed boundary" true in code and not just in prose — the loop cannot chase an appender. `verify_source` re-checks device/inode and refuses any length below the observed end, so rotation and truncation invalidate the whole call rather than just the checkpoint; same-inode equal-length rewrite is explicitly named unsupported in both source comment and `fidelity.md`. Blank framing is `u8::is_ascii_whitespace` over the physical line, budget-consuming and offset-advancing but not passed to the mapper — matching the B3 disposition I asked for at baseline. Oversize handling distinguishes correctly: capacity clamped by `max_record_bytes` yields `RecordLimit` with no cursor, while capacity clamped by remaining batch budget yields `more = true` with progress intact.

**Adapter (`ac-0004`, `ac-0005`, `ac-0008`) — physical/logical separation is real.**
One `TelemetryRecord` per `NativeRecord`, `BTreeMap` attributes and in-order diagnostics give byte-identical output for equal input. Repeated `message.id` (`response-shared` across two fixture records) produces two records; nothing deduplicates. No provider name, no usage totals, no trace/span IDs are invented — and the proof harness actively fails on any `gen_ai.usage.*` key or `traceId`/`spanId` field, so that is enforced, not merely intended. Usage components are retained individually with `unisphere.usage.scope = native_record_snapshot`, which is the honest framing. Metadata-only genuinely emits `body: None`; structural diagnostics (`UnsupportedPart`, `InvalidField`) are still produced in metadata mode, so policy omission does not blind the operator to shape. `thinking.signature` and `toolUseResult.persistedOutputPath` are never read, let alone opened. Malformed JSON fails the whole batch at its physical offset with fixed public copy — `PipelineError` carries only a closed kind plus an offset, so no native byte can reach a message.

**Writer (`ac-0006`) — the bound is on the right representation.**
Encoding stages into a capped `BatchBuffer` and the destination is untouched until the full batch is encoded, so `OutputLimit` and `InvalidData` both precede any partial write. The 32 MiB cap counts the actual OTLP bytes including JSON escape expansion and the terminating LF, which is exactly the split I argued for at baseline (B2) and which `docs/adapters.md` now states explicitly. `serde_json::Error → io::Error` preserves the inner `FileTooLarge`, so an over-budget string mid-encode still classifies as `OutputLimit` rather than being mislabelled `InvalidData`. Integers outside `i64` are rejected instead of rounded or stringified; `Value::Null` maps to `{}` rather than a fabricated variant. `resource`, `schemaUrl`, `observedTimeUnixNano`, severity and trace fields are omitted rather than invented, and `telemetry-profile.md` states plainly that an omitted observed-time must be established downstream.

**SDK and CLI (`ac-0001`, `ac-0007`, `ac-0009`, `ac-000a`).**
`collect_batch` validates limits and session, re-validates the loader's `LoadedBatch` against those limits, and then independently rejects a non-monotonic cursor, a foreign source, records before the start offset, an identity change across a cursor, and — importantly — `more == true` with zero progress. That last guard is what makes the CLI drain loop terminating even against a hostile or buggy injected loader; the CLI repeats it. The next cursor is returned only after `writer.write_batch` succeeds, so no failure path can advance a checkpoint. Telemetry goes to stdout or a `create_new` file; summary and typed errors go to stderr; the proof confirms an existing output file is left byte-identical on refusal and that malformed input produces empty stdout with no marker on stderr. `crates/app/src/adapters.rs` registers `claude-code` in one const table, and its test composes `TextFixtureAdapter` through a second registration with no change to core or the Claude adapter — `ac-000a` proved by construction rather than asserted.

**Regressions and dependency policy (`ac-000b`).**
`allowed()` in `unisphere-arch-check` is exactly the C12 graph, with the shared dev-only arm (`unisphere-testkit`, `tempfile`, `serde_json`) confined to `dev`; no production edge reaches testkit. Internal edges must be local paths. The C2 zero-core-scan guard I raised at baseline is now implemented — `core_count == 0` is a hard error. `purity.json` covers fully qualified and grouped imports, clock, include macros and unsafe as negatives, with core ports and `#![forbid(unsafe_code)]` as positives. No `set_var`/`remove_var` and no `HOME`/`dirs` lookup exists anywhere under `crates/`; the proof additionally re-runs the external consumer under hostile `HOME`, `XDG_CONFIG_HOME`, `UNISPHERE_CONFIG` and `CLAUDE_CONFIG_DIR` and requires byte-identical stdout.

**External consumer and installed CLI (`ac-000c`).**
`crates/testkit/src/bin/proof/collection.rs` builds a fresh out-of-tree consumer against the four crates by path, builds and `cargo install`s the real binary to a temporary root, and requires the installed CLI, the in-tree CLI and the external SDK to agree byte-for-byte in both policy modes at `--max-records 1` — six records from a three-record fixture, a blank ` \r\n` line and a three-record fixture, which simultaneously proves blank-line skipping, physical retention across repeated logical IDs and multi-batch drain.

## AC coverage

| AC | Verdict | Load-bearing evidence |
| --- | --- | --- |
| ac-0001 injectable ports, deterministic mapping | met | `Collector::new`, `CollectionApi`, `assert_adapter_conformance`, replay/split-batch adapter tests |
| ac-0002 deterministic bounded listing, no HOME scan, no symlink follow | met | sorted non-recursive listing, `O_NOFOLLOW`, symlink/FIFO tests, hostile-env proof |
| ac-0003 resume, blank progress, partial tail, source change, typed limits, Unix-only | met | LF-boundary cursor check, `verify_source`, `incomplete_tail`, `RecordLimit`, `Unsupported` on non-Unix |
| ac-0004 message/part/identity/usage mapping without fabrication | met | 15 adapter tests, `unisphere.usage.scope`, proof rejects `gen_ai.usage.*` |
| ac-0005 diagnostics not panics, no echoed bytes | met | closed `MappingDiagnosticCode`, offset-only errors, malformed-input proof |
| ac-0006 valid OTLP JSONL, extensions clearly named | met | writer tests, `telemetry-profile.md` registry, operator smoke output |
| ac-0007 checkpoint only after output acceptance | met | `collect_batch` ordering plus five independent post-load guards |
| ac-0008 metadata-only default, no implicit dereference | met | `body: None` in default mode, spill path never read, proof asserts no `SENSITIVE-` |
| ac-0009 list/export, stdout not corrupted, no overwrite | met | `create_new`, stderr summary, existing-file and malformed-input proof cases |
| ac-000a second adapter via one registration | met | `another_adapter_needs_one_registration_and_no_core_or_claude_change` |
| ac-000b compatibility, dependency checks, test isolation | met | 6 harness gates, arch/purity sensor, no env mutation anywhere |
| ac-000c external consumer + installed CLI | met | `unisphere-proof collection` exit 0 |

## Fidelity report judgement

`docs/fidelity.md` is judged as the requested delivery report and it holds up. All six dimensions are present with a full-fidelity definition, implemented behavior, named proof and a **classified** gap — the classification vocabulary (intentional policy / unsupported behavior / source limitation) is what stops the matrix collapsing into an apology. Three specific things earn it:

1. It states the non-lossless position in the first sentence and repeats it at the CLI-experience section, rather than burying it.
2. Its evidence section names the non-UTF-8 candidate branch as **NOT EXERCISED** on this filesystem instead of letting a green test count imply platform coverage.
3. The follow-on list is scoped as product follow-ons, explicitly not new completion barriers, which matches the operator's recorded clarification in `full-fidelity-requirement.json`.

Cross-checked against source, its claims are accurate: unknown-field retention really is absent, revision/delete history really is absent, `more`/`incomplete_tail` really do describe only the current read, and the CLI really does report final offset and mapped count rather than a from/to ordinal range.

## Findings

All four are advisory. None blocks; each is recorded `accepted` with rationale rather than left open.

**D1 — duplicated argv parse for adapter selection.** `crates/cli/src/sessions.rs:52` scans argv for `--adapter` independently of clap so the composition root can build the collector before parsing, and the export summary's `adapter` field then comes from clap's parse. Today they cannot disagree on any clap-accepted invocation: both take the last occurrence and both accept the `=` form. The residual risk is future drift — an adapter-affecting flag, `allow_hyphen_values`, or a positional form would let the summary name a different adapter than the one that actually mapped. Smallest fix, only if that day comes: expose the composed adapter's `name()` through `CollectionApi` and have `run_sessions` report that instead of the parsed string. *Accepted:* the seam is inherent to selecting an implementation before parsing, the obligation is written at both ends, and the app-registry test exercises the real path.

**D2 — unmapped top-level native fields are dropped without a diagnostic.** `toolUseResult`, `cwd`, `gitBranch`, `version`, `requestId` and `userType` are neither exported nor counted; only invalid *recognised* fields raise `InvalidField`. *Accepted:* this is the documented projection boundary, not a defect — `claude-adapter.md` states outer `toolUseResult` is not interpreted or exported, and `fidelity.md` classifies complete unknown-field retention as unsupported behavior. Raising it as a gate would contradict the operator's recorded decision that fidelity is a report at delivery, not new scope.

**D3 — a green test that proves nothing on this filesystem.** `non_utf8_candidates_are_rejected_when_filesystem_permits_them` passes on APFS by printing `NOT EXERCISED` when `fs::write` returns `EILSEQ`. A reader counting passing tests would over-read platform coverage. *Accepted:* the alternative shapes are worse — a hard failure would break the lane on the developer platform, and `#[ignore]` would hide it. The honest disclosure exists in three places (`stderr` notice, `tk-0002-validation-corrected.json`, `fidelity.md`), and the before-I/O non-UTF-8 rejection path *is* exercised by `non_utf8_roots_and_inputs_are_rejected_before_io`.

**D4 — `encode_value` recursion depth is a caller obligation, not a writer bound.** The writer caps encoded bytes but not nesting, and each level writes only ~24 bytes, so the 32 MiB cap does not bound stack depth. *Accepted:* unreachable on the shipped path — records arrive via `serde_json::from_slice` at default features (`1.0.151`, no `unbounded_depth`), so input nesting is capped at the deserializer's limit long before the writer sees it. Only an in-process injected adapter constructing a `Value` directly could exceed it, which is the same trust domain as the composition root. If a hostile-adapter threat model is ever adopted, the fix is a depth counter in `encode_value` returning `InvalidData`.

## Gaps in this review

- I executed nothing. Every runtime result — 123 workspace tests, 6 quality gates, `unisphere-proof collection`, `harness checks`/`boot`, operator export — is read as recorded evidence produced by the PM, not reproduced by me.
- The lexical purity sensor's ceiling applies to my reading too: I inspected imports and call sites in the pure crates directly, but neither the sensor nor a source read is a formal effect system.
- I did not review `.dd/schemas/builder/work-packet/schema.json` (untracked, unmapped ownership) or re-audit the frozen baseline beyond confirming its eight digests are unchanged.
- Prior baseline lows accepted at `ac4d28f6` (C1 fixture edge count, C2 zero-core guard, C3 dev-only arm) were not relitigated; C2 is now implemented in the checker.
