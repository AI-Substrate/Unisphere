# Unisphere — SDK/CLI requirements spine

**Status:** Requirements captured; Jordan authorized Plan 001 product planning for foundation-only delivery. This spine is input to the plan, not an implementation guide, assignment or authorization to implement.
**Owner of intent:** Jordan (operator).
**Capture:** `pij-female-varl`, 2026-09-07.
**Workspace:** `/Users/jordanknight/substrate/unisphere/unisphere-sdk-cli-foundation`.
**Branch:** `builder/001-sdk-cli-foundation`.
**Builder allocation:** `al-001-4437fdce-9332-4eed-9398-7651d0ab20e2`, from main commit `aa5e75cad1d8397a7ec392e2cb7773c00f673a93`.

The first-class `harness builder new` command generated plan/guide/task/flow scaffolding in this folder. Those files were not approved by allocation. Jordan subsequently authorized authoring the initial product plan and selected foundation-only scope (RQ-014–017); the example implementation guide remains unapproved. This spine preserves the conversation as planning input and does not itself advance flow state or release implementation.

## Confirmed requirements and instructions

| ID | Requirement / instruction | Basis |
|---|---|---|
| RQ-001 | Build Unisphere in Rust, usable both as a standalone CLI and as an in-process library/SDK included by other Rust programs. | Initial product ask. |
| RQ-002 | Read native agent-harness session/telemetry formats through adapters and expose a common format that downstream tools can consume without maintaining their own dialect parsers. | Initial product ask; Claude Code, Copilot and Cursor were named examples. |
| RQ-003 | Survey the different harness formats and existing readers before finalizing the common model or adapter scope. | Initial research request and handover. |
| RQ-004 | Begin product work with a focused initial effort establishing the base SDK, CLI, code-composition architecture and meaningful tests. | "an initial plan that gets teh base cli and sdk in place" and subsequent architecture/tests request. |
| RQ-005 | Keep services and implementation details from leaking across boundaries; dependencies must point one way, with explicit injection/IoC. | Operator's architecture discussion. |
| RQ-006 | Reuse knowledge and useful code from git-ai, but rewriting it to fit Unisphere's model is acceptable; there is no requirement to preserve git-ai's architecture. | "i'm happy if we re-write it to fit our model." |
| RQ-007 | Use Flowspace3's Rust layout, services, adapters and testing approach as architectural reference material. | Operator requested independent structural reviews. |
| RQ-008 | Use Builder for the eventual plan/guide/tasks/implementation lifecycle, and dogfood the new first-class harness commands for product worktree creation. | Explicit Builder and first-consumer dogfood instructions. |
| RQ-009 | Persist governance on a permanent orphan prime-governance branch/worktree and seed AGENTS.md so new primes can discover and read it. | Governance discussion and setup approval. |
| RQ-010 | We are still collecting requirements; create this requirements spine in the allocated plan folder, not a completed plan. | Explicit correction during setup. |
| RQ-011 | Research best practice and industry conventions for the common format, beginning with Perplexity research on whether OpenTelemetry has a common file format for agent telemetry. | Latest research request. |
| RQ-012 | Report Builder dogfood progress and problems to harness prime `pij-varied-alpaca`. | Explicit current peer designation; supersedes older registry names. |
| RQ-013 | Flowspace3 is the first consumer of the Rust SDK; interview `pij-binding-magpie` for requirements without allowing one consumer to dictate the general-purpose model. | Explicit first-consumer instruction. |
| RQ-014 | Author the initial Builder product plan now; this explicitly supersedes RQ-010's earlier collection-only stop without authorizing implementation. | "lets get a /builder plan on it." |
| RQ-015 | Plan 001 is foundation only: SDK, CLI, real shared configuration/diagnostic behavior, dependency boundaries, packaging and tests; no first native-session reader in this plan. | Operator selected "Foundation only" in scope clarification. |
| RQ-016 | Primes author the initial product plan; PMs subsequently orchestrate implementation with their peers. | Explicit operator ownership instruction. |
| RQ-017 | Services and other independent components can be built by separate agents against agreed contracts and composed later; a finished CLI must not be a prerequisite for service development. | Explicit operator fan-out instruction during planning. |
| RQ-018 | Native-session reader experiments may proceed independently as Rust services/adapters under gitignored `scratch/`, for later integration when ready; they do not expand Plan001 shipping scope. | Explicit operator experiment authorization. |
| RQ-019 | Run a Builder workshop on common output format and standard fields; no format has yet been selected. | Explicit output-format workshop request. |
| RQ-020 | The common output must carry available pij peer names/identifiers, roles and related lineage/context, with honest provenance and unknowns rather than conflating them with model/provider identity. | "need pij names and roles etc in there too." |

## Working architectural direction — not a frozen design

The proposed names are **hexagonal architecture / ports and adapters**, **functional core / imperative shell**, **dependency inversion**, **constructor injection**, and an explicit **composition root**.

- The Rust SDK owns application operations; the CLI calls the SDK rather than duplicating its behavior.
- Application services depend on inner domain contracts; source adapters implement those contracts and depend inward.
- Pure domain data and normalization rules do not depend on CLI rendering, database handles, a daemon or an async runtime.
- Wiring selects concrete implementations explicitly; no hidden service locator or global mutable dependency graph.
- Traits are justified at genuine variation points; crate count, exact signatures, sync/async policy, public exports and feature flags remain undecided.
- Shared source-adapter contracts, dependency-direction checks and a real composed SDK/CLI scenario are proof candidates; their exact acceptance criteria belong to the future plan/guide.

No Rust product code or tests have been implemented. Harness onboarding tests do not prove a working SDK or collector.

## Product research context to preserve

These are findings and design pressures, not silently adopted requirements:

- git-ai's session stream preserves native JSON inside a common envelope; its attribution transcript and Claude/Codex usage pipeline normalize subsets, not the complete intended session contract.
- Flowspace3's source-reader seam is useful, but its search-oriented projection intentionally drops or truncates information; do not inherit those losses as a universal format by accident.
- Physical records, logical messages, requests and turns are different units; JSONL inputs can encode updates and rewinds, not only new messages.
- Cursors, replay/deduplication, record revisions, subagent lineage, usage scope, missing values and timing precision need explicit decisions.
- Full-content and counts-only outputs, strong privacy boundaries and OTLP export/import are candidates to evaluate; they are not yet finalized product profiles.
- A canonical format does not imply a mandatory central server, database, daemon or ML runtime.

## Open decisions

| ID | Question | Status |
|---|---|---|
| Q-001 | Is there a standard OpenTelemetry on-disk file format for complete agent telemetry/session history, as opposed to a wire encoding or exporter convention? | Research found an official Development-stage OTLP JSONL file spec, but no complete native-session replay contract in the reviewed specs; see OF-001–008 in the research report. |
| Q-002 | Should the canonical persisted representation be OTLP JSON/JSONL, a purpose-built session/event model with an OTLP mapping, or another established representation? | Open; compare semantics, stability, replay and consumer ergonomics. |
| Q-003 | What must the initial SDK/CLI foundation actually do end to end, and which first adapter/fixture proves it without attempting the whole survey's ecosystem? | Open; avoid both empty scaffolding and unapproved scope expansion. |
| Q-004 | What content, metadata, usage, lineage, lifecycle, update and rewind semantics must the common model preserve? | Open. |
| Q-005 | What privacy modes and consent boundaries are required, including content export versus content entering the process? | Open. |
| Q-006 | Which consumers own scheduling, cancellation, cursor/parser-state persistence and recovery, and what guarantees does the SDK provide? | Open. |
| Q-007 | Which Rust crate/module boundaries, optional dependencies and public API stability guarantees are appropriate? | Open; architecture names are guidance, not a fixed crate graph. |
| Q-008 | Which current native harness versions/storage variants are initial delivery targets versus later adapters? | Open; named examples are not a final release-support matrix. |

## Evidence and pointers

- Governance entry: `/Users/jordanknight/substrate/unisphere/unisphere-governance/AGENTS.md` and `.harness/government/orient-local.md`.
- Governance brief: `/Users/jordanknight/substrate/unisphere/unisphere-governance/.harness/government/briefs/sdk-cli-foundation.md`.
- Prior handover: `/Users/jordanknight/substrate/flowspace/flowspace3/scratch/brief-harness-telemetry-standardiser.md`.
- Prior code references: `/Users/jordanknight/github/git-ai` and `/Users/jordanknight/substrate/flowspace/flowspace3`.
- Flowspace governance decisions: `conv:f3a6f4d9-f037-864a-a824-c436aa5febb2#t7748` and `#t8580`, retrieved with `flowspace3 get`.
- Research output belongs under this folder's `assets/research/`; link verified findings here without changing a candidate into an approved decision.
- OpenTelemetry file-format research: [assets/research/otel-common-format.md](assets/research/otel-common-format.md); findings/candidates only, no format choice.
- First-consumer interview: [assets/requirements/flowspace3-interview.md](assets/requirements/flowspace3-interview.md); Flowspace3 needs must be separated from consumer-local policy.

## Dogfood log

| Observation | Outcome |
|---|---|
| Relative sibling --workspace path refused with E477. | Followed the command's explicit absolute-path remedy; no manual Git product allocation. |
| Absolute-path harness builder new. | Succeeded; allocation, plan folder and canonical flow created by the harness. |
| Requirement-collection stage versus populated example guide. | Harness prime confirmed the teaching guide copied by new is a defect, backlog row 37; leave it unapproved and use this requirements spine. |
| Repo-local ddocs missing; npm proxy 404 and direct registry ENOTCONN. | Ignored local tooling links use the installed harness's bundled dd 0.1.0; no global config or Rust runtime dependency changed. |
| Earlier harness-prime registry identity could not receive messages. | Operator supplied pij-varied-alpaca; future reports use that identity. |
| Builder ready on generated scaffolding returned E471/not-ready. | Correct refusal: product ACs are empty and example guide links refer to unknown criteria; schema existence is not approval. |
| Legacy pij state --json refused E-RS for an rs seat. | Harness prime identified an intentional legacy-projection boundary; use native pij-rs state for the rs schema, not a fabricated legacy shape. |

## Change discipline

Append new requirements and questions with stable IDs; preserve corrections instead of silently rewriting intent. Record operator decisions separately from research recommendations. When requirements are ready, derive the product plan and implementation guide through Builder with links back to this spine; do not advance the flow, seal contracts, dispatch coders or mark criteria proven based on this document alone.
