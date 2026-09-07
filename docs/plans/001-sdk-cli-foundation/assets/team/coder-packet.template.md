# Coder packet template

This is an editable briefing template, not an issued `builder/packet` record or implementation release. The PM fills the binding table from the canonical dispatch record; the dispatch writer creates the immutable DD packet and its digest. Never invent identities, hashes, ownership or runtime observations.

| Binding | Fill from |
|---|---|
| Unit and responsibility | reviewed implementation guide unit |
| Native peer and workspace root | observed allocation/runtime binding |
| Parent | governing PM identity |
| Source baseline SHA | sealed baseline `source_sha` |
| Canonical packet path, SHA-256 and nonce | actual dispatch result |
| Plan, guide and allocation | packet's path/digest bindings |
| Requested harness, model and optional effort | resolved role plus field provenance |
| Native-root challenge | packet's relative `canary.path`; never include its answer |

## Scope

Copy the complete assigned unit's write fence, explicit reads/owners, frozen interface, dependencies/wave, acceptance links and proof commands. Include every scoped supplement. Reuse shared contracts and injected adapters/fakes; do not import a same-wave sibling implementation or renegotiate the interface silently.

## Pre-work and post-release acknowledgements

Before work, read the canonical packet through the native repository-relative file tool. Verify the pristine source baseline, packet digest, native root, shell cwd, peer identity/session/PID and actual harness/model argv/environment where available. Read the clone/worktree-only canary by its relative native file-tool path; a shell directory change does not prove native-root binding.

Return the exact `AckReceipt` fields: `record_type`, `id`, `recorded_at`, `unit_id`, `peer_id`, `nonce`, `packet_sha256`, `baseline_sha`, `native_root`, `shell_cwd`, `canary_nonce`, `observed`. `baseline_sha` is the full Git source SHA, not the baseline file digest. Unsupported observations go in `observed.gaps`; omitted effort stays absent and provider-served identity remains unverified.

Use `ack-<unit_id>-<full-current-source-sha>` for the pre-work receipt, with `nonce = packet.nonce`. The current guide-bound verified seal selects the source, not a caller-supplied old delivery. Send the new private receipt path and SHA-256; the PM ingests it with `harness builder ack <plan> --receipt <path>`. No work until you observe the exact-bound explicit release. Queued does not mean received.

After observing that release, natively re-read packet/canary and refresh runtime observations. Write a **distinct new** raw `AckReceipt` with ID `ack-<unit_id>-<full-current-source-sha>-release`, `nonce = release.message_id` from the already-recorded exact release, and your actual new receipt creation time. Do not derive this phase's nonce from `packet.nonce`, even when their values happen to match. Preserve the original pre-work receipt. All unit/peer/packet/source/root/canary bindings still apply; source ancestry and allocated branch remain checked, but authorized work need not still be pristine or at the baseline HEAD.

Send the post-release receipt's new private path/SHA-256, then follow the **already-granted** scope without waiting for a second grant. If the work is already complete, confirm the real retained release without replaying it. The PM submits the fresh receipt with the same `harness builder ack <plan> --receipt <path>` before importing a queued delivery. Confirmation grants/sends nothing. Retrying the initial receipt cannot promote queued to delivered; neither a filename nor a loose ID prefix identifies the phase.

Use valid actual `recorded_at`, with the explicit **5000 ms** peer-clock skew tolerance: no earlier than the retained release sent time minus 5000 ms and no later than PM ingestion time plus 5000 ms. PM records both observation and ingestion times in `observed.evidence`; do not fake a timestamp to fit. Invalid/out-of-window time needs clock-skew or incorrect-receipt guidance, never a silently wider tolerance. Omit unavailable optional runtime fields and name gaps; configuration is not provider attestation.

Missing current-qualified authorization, an older-qualified record, wrong phase/binding or changed immutable confirmation requires a named refusal, never a unit-only fallback or manual delivered flag. Identical confirmation retries are safe without resending a release. Keep every original packet, acknowledgement and release record unchanged.

## Boundaries

Write only the declared source/test fence. Do not edit `.the-flow-state.json`, `the-flow.json`, `the-flow.md`, canonical plan/guide/task/receipt state, other workers' paths, main, global/deployed skills or settings. No push/PR/merge or unrelated deletion. The PM states validation ownership explicitly; do not run project-wide checks or generators while independent units are still in flight. Ask the PM before changing frozen interfaces.

## Return

Commit only the declared source/test paths through the repository's scoped commit surface. Return `UnitDelivery`: `unit_id`, `peer_id`, `workspace`, `commit_sha`, `packet_sha256`, `baseline_sha`. Attach exact exports, changed paths, authored-versus-executed checks, complete acceptance dispositions, output/evidence pointers, risks, friction and one encodable improvement. Preserve attribution warnings. Import is not composition proof; the PM wires and verifies the committed composition.
