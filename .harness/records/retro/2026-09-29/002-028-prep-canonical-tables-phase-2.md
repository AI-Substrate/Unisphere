---
record_kind: "retro"
harness_version: "0.14.0"
branch: "builder/028-prep-canonical-tables"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-29T09:58:18.455Z"
agent: "pij-native-tick"
plan_id: "028-prep-canonical-tables"
schema_version: "1.2"
retro_id: "2026-09-29T09:58:54Z-pij-native-tick-p2drain"
started_at: "2026-09-29T07:15:50.005Z"
ended_at: "2026-09-29T09:58:54Z"
summary: "Plan 028 phase-2 drain (PM): Builder multi-phase friction (advance gate, dispatch readiness, second import, guide/plan drift rules, ownership prefixes, receipt custody), one packet-map gap and the real-corpus findings that drove the prime's two rulings; one win for the numbers-only proofs. All entries kept; harness-itself entries await the operator's decision on upstream issues."
entries:
  - id: DL-001
    kind: difficulty
    description: "harness builder advance review-1 -> phase-2 refuses E475 'Composition is imported but not verified against the current baseline' once guide v3 is re-sealed for phase 2: the departure gate compares the phase-1 composition with the new phase-2 baseline, so a multi-phase plan cannot re-seal before dispatch and still advance; nav stayed on review-1 until the phase-2 composition verified."
    target: harness-itself
    severity: degrading
    workaround: "Left nav on review-1 through phase 2; advanced review-1 -> phase-2 after the phase-2 composition verified and was reviewed."
    suggested_encoding: "Let advance evaluate the departing phase against the baseline its composition was verified on (per-phase baselines); file upstream on AI-Substrate/harness-engineering."
    fp: "54648b5ab76e"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T07:15:50.005Z"
  - id: DL-002
    kind: difficulty
    description: "harness builder dispatch readiness treats every transitive depends_on unit whose proof checks are not in the baseline proof as in-flight; phase-2 lanes depending on delivered phase-1 units were refused E471 after the seal."
    target: harness-itself
    severity: degrading
    workaround: "Phase-2 lanes depend only on the contract unit tk-0001; guide re-reviewed (p2 r2) and re-sealed."
    suggested_encoding: "Treat units delivered and verified in an earlier composition as satisfied dependencies; file upstream."
    fp: "bbb3bc0d9575"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T07:16:54.207Z"
  - id: DL-003
    kind: difficulty
    description: "harness builder compose --import refuses E472 'A composition receipt already exists' for the second phase of a multi-phase plan; the phase-1 receipt had to be git-mv'd to composition-p1.dd.* (harness commit naming the removed path failed E130) before phase 2 could import."
    target: harness-itself
    severity: degrading
    workaround: "Renamed the phase-1 receipt to composition-p1.dd.{json,md} and committed only the new paths."
    suggested_encoding: "Per-phase composition receipt paths (or archive on re-seal); file upstream."
    fp: "065192fddf41"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T09:45:10.251Z"
  - id: DL-004
    kind: difficulty
    description: "Builder basis drift treats every impl-guide change except meta.updated as material; prime rulings made during composition (new contract fields, resolved risk rk-000b) could only be recorded in meta.updated prose, leaving the risks/architecture sections stale so the sealed baseline and reviews stayed valid."
    target: harness-itself
    severity: degrading
    workaround: "Recorded the composition delta in meta.updated (dfb8bf8) and in the execution log."
    suggested_encoding: "An additive guide 'composition_deltas' section excluded from intent drift; file upstream."
    fp: "27c3ac0a0050"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T09:45:10.419Z"
  - id: DL-005
    kind: difficulty
    description: "builder on-track and compose ownership checks treat a directory path in a unit's paths as an exact file, so every file under mapped fixture/recipe directories is flagged owning_unit unmapped (reported by coders on tk-0007, tk-000a and tk-000d; 124 warnings at phase-2 verify)."
    target: harness-itself
    severity: degrading
    workaround: "Ignored the warnings after checking each flagged path lies under its unit's declared directory."
    suggested_encoding: "Prefix-match directory entries in unit paths; file upstream."
    fp: "6f61e97143f9"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T09:45:10.763Z"
  - id: DL-006
    kind: difficulty
    description: "compose --verify records artifact_sha in a descendant commit of the verified artifact, so a reviewer checking out the exact subject sees the previous verification in composition.dd.json; the reviewer flagged it as a chain-of-custody gap."
    target: harness-itself
    severity: annoying
    workaround: "Reviewer packet carries a chain-of-custody row naming the receipt commit and its parent."
    suggested_encoding: "Have compose --verify print the receipt commit and packets name it; or accept the receipt commit as review subject."
    fp: "2526d908e7fb"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T09:45:18.873Z"
  - id: DL-007
    kind: difficulty
    description: "Adding a receipt field to a checked plan phase row (the phase-1 pattern) is material plan drift for the sealed baseline: advance --now phase-2 refused E475 'Material document drift: plan.dd.json' because the intent projection excludes only state/proven_by."
    target: harness-itself
    severity: annoying
    workaround: "Dropped ph-62fe.receipt; the review is linked from lg-0011 and the team review record."
    suggested_encoding: "Exclude phase receipts from intent drift like state/proven_by; file upstream."
    fp: "1fa2216d8f3d"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T09:58:13.107Z"
  - id: DL-008
    kind: difficulty
    description: "Coder tk-000b: the work packet's path map omitted crates/adapter-cursor/src/ide.rs and query.rs, but the lane needed ide.rs (cli_persisted_resume) and a pub(crate) on query::tool_family; the PM authorised both mid-flight."
    target: plan
    severity: annoying
    workaround: "PM ruling sent to the coder; out-of-map edits recorded at import."
    suggested_encoding: "Guide authoring: include each lane's adapter descriptor/shared vocabulary files in its path map."
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T08:00:00Z"
  - id: DL-009
    kind: difficulty
    description: "Phase-2 real-corpus proofs exposed three gaps the synthetic lanes could not: 15 sources with records up to 13.3 MiB unreadable under the 3 MiB collection default; warm re-runs at 49.5 s from re-walking 14,707 directories; cold peak RSS 3.97 GB from buffering a whole run's rows. All fixed in composition (record limit = batch limit, incremental discovery, commit waves) under prime rulings."
    target: plan
    severity: degrading
    workaround: "Escalated with numbers; implemented the rulings in composition with tests and re-measured."
    suggested_encoding: "Run the real-corpus coverage/measure scripts at lane delivery (numbers only) rather than only at composition."
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T08:20:00Z"
  - id: WIN-001
    kind: win
    description: "Numbers-only real-corpus sensors (coverage, measure with CPU/footprint/dirs listed, live, parity, recipes) turned every AC-0006/0009/000c claim into rerunnable evidence; the independent reviewer re-ran the boot prep proof and 150 tests in its own clone and approved with 0 findings."
    target: project
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T09:58:54Z"
system:
  compound:
    bubble_action: "all-save"
---

# Retro — Plan 028 phase 2 (PM pij-native-tick)

Highest-value encoding: DL-001/DL-003 together — Builder should model per-phase baselines and composition receipts so a multi-phase plan can re-seal, import and advance without renaming receipts or parking the flow cursor. Second: DL-009 — run the numbers-only real-corpus scripts at lane delivery, not only at composition.
