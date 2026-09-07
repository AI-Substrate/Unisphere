# Flowspace3 first-consumer interview

**Status:** Awaiting `pij-binding-magpie` response; no interview answers or consumer proposals have been received or approved as Unisphere requirements.
**Interviewer:** `pij-female-varl`.
**Consumer contact:** `pij-binding-magpie`.
**Operator instruction:** "Flowspace3 will be the first consumer of our SDK. interview pij-binding-magpie and collect requiremnts, although dont let tail wag dog."

## Context and boundary

Unisphere is a standalone Rust SDK plus thin CLI for reading native agent-harness telemetry/session stores into a common model. We are collecting requirements only; no product plan, implementation guide or SDK API is approved. The initial Builder workspace is `builder/001-sdk-cli-foundation`; `requirements-spine.md` is the intent capture. We read your handover at `/Users/jordanknight/substrate/flowspace/flowspace3/scratch/brief-harness-telemetry-standardiser.md` and reviewed Flowspace3's current source seam and normalization policy.

Flowspace3 is the first consumer, not the owner of the general-purpose contract. Its PostgreSQL schema, queue, summarization policy, tool-output caps and UI conventions must not silently become SDK requirements. Please mark necessities, preferences and consumer-local behavior separately. Do not edit Unisphere files; reply through pij with findings and source/fixture pointers, not private session content.

## Questions for magpie

1. What concrete Flowspace3 user behavior should the first Unisphere integration unlock or preserve, and which existing reader/orchestration responsibilities would the SDK replace versus leave in Flowspace3?
2. Which capabilities are essential for the first usable integration: discover sessions, resolve files/sidecars, read a complete session, incremental updates, lookup by native identity, or something else? Name required input/output semantics, not a preferred function signature.
3. What data must be preserved for Flowspace3: message identity/order, tool calls/results, sidecar lineage, source paths, timestamps/precision, usage/model, compaction, branch/rewind events and opaque unsupported content? Which are required immediately versus desirable later?
4. What exact retry/resume/replay guarantees does Flowspace3 need to avoid paying for duplicate summarization? Who should own cursor/parser state, persistence transactions, scheduling, cancellation, deduplication, late updates and empty-read acknowledgements?
5. What constraints affect embedding: synchronous versus async calling, bounded batches/memory, optional SQLite/native dependencies, platform support, read-only producer stores and privacy? Distinguish hard constraints from preferences.
6. Which of today's Flowspace3 choices must remain consumer-local (512-byte tool-result cap, dropping thinking, write-input elision, PostgreSQL turn schema, queue/liveness thresholds, model pricing/indexing), and which actual fixture/scenario would prove a successful first integration?

Please return a ranked set of must-have, preferred and out-of-SDK responsibilities, with exact current source/fixture pointers and any migration hazards. Do not assume the current frozen ConversationSource/Turn shape is the new public SDK API, and do not turn first-consumer compatibility into a universal schema decision.

## Delivery evidence and next action

- Initial interview request accepted by native `pij_send`: `2e316644-1efa-4735-849b-ba7d4cc4bc45`.
- Acknowledgment request accepted by native `pij_send`: `a75711e2-d9d9-40f2-ba81-b344630a286b`.
- Native `pij-rs state pij-binding-magpie --json` reported active/idle at the Flowspace3 checkout; this is liveness evidence, not proof that the interview was read.
- The legacy `pij state --json`/`pij tail` surfaces could not project/find the rs seat; the harness prime explained the legacy/native contract distinction. No transport files were read or replayed.
- Await the contact's reply, then distinguish shared SDK invariants, Flowspace-only policy and optional preferences; ask follow-ups only for material gaps. Existing source research and the handover do not substitute for the requested interview.
