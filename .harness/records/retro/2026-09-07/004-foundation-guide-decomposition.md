---
record_kind: "retro"
harness_version: "0.14.0"
branch: "builder/001-sdk-cli-foundation"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-07T04:31:47.697Z"
agent: "pij-female-varl"
plan_id: "001-sdk-cli-foundation"
schema_version: "1.2"
retro_id: "2026-09-07T04:33:50Z-pij-female-varl-7d0495ba6c26"
started_at: "2026-09-07T04:00:27.579Z"
ended_at: "2026-09-07T04:33:50Z"
summary: "Replaced the teaching guide with exact foundation contracts, PM/coder ownership, proof and native Builder composition flow; actual OMP Opus5/high review found nine gaps, all corrected and independently approved in r2. Builder recorded both changes-requested and approved receipts; implementation readiness remains false until baseline code exists."
entries:
  - id: DL-001
    kind: difficulty
    description: "Teaching guide capabilities use cp-* IDs but ddocs --mint cp refuses E454 because cp is unregistered."
    target: tooling
    severity: degrading
    workaround: "Keyed one-to-one capability rows by the existing writer-minted product AC IDs; no schema or tooling modification."
    suggested_encoding: "Align Builder templates and registered DD prefixes; harness prime confirmed backlog row 40."
    fp: "7d0495ba6c26"
    disposition: kept
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T04:00:27.579Z"
  - id: INS-001
    kind: insight
    description: "A real reviewer emitted observed.identity and string argv; Builder refused E470 until the reviewer reissued peer_id and argv[] at a new path."
    target: tooling
    disposition: kept
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T04:18:16.458Z"
---

# Retro — Foundation guide decomposition

- PM is operator-designated `pij-right-kotallo`; coders are OMP GitHub Astra/high, independent reviewers OMP Claude Opus5/high. Actual reviewer `pij-angry-hekarro` passed native-file/root/argv/registry canary; provider-served identity/effort is not claimed.
- Guide structural checks validated 5 units, 3 independent coder lanes, 11 AC mappings and 15 planned commands. No product code/gates were executed or invented.
- First review requested changes: public constructors, Cargo fixture templates, lockfile ownership, ambient/no-network evidence, actual toolchain checks and four smaller contract clarifications. All nine were fixed; r2 approved decomposition at `d325a8f60a3c97739b7c3b16e612110b5db84530` with no open findings.
- Native Builder review ingestion recorded the historical changes-requested receipt and then the approved r2 receipt; original reports and basis snapshots remain intact. No parent patching of reviewer evidence or gate bypass occurred.
- Remote peer compact was attempted as the Pij skill directs but the native send surface refused remote controls; no typed-command fallback or false compaction claim. Native event subscription was used while awaiting review and stopped afterward.
- The guide checker rejects all overlapping fences, including sequential PM units. A single composition-owned generated lockfile plus explicit baseline-preparation authorization is the reviewed workaround; probe actual sealing behavior and report disagreement rather than bypass it.
- Current Builder readiness returns E471 for missing baseline Cargo.toml, as expected before implementation. Mixed host compiler/components are recorded as an execution prerequisite; rust-toolchain.toml alone is not proof.
- Highest-value future improvement: typed first-class reviewer receipt preparation/validation plus template/prefix parity, so an independent reviewer need not infer the wire shape. Experience reported to `pij-varied-alpaca`.
