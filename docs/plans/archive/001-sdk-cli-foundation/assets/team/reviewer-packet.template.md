# Reviewer packet template

This is an editable review briefing template, not an issued review receipt or proof that a reviewer ran. The PM fills every binding from observed records before delivery. Missing requested review capability remains unfulfilled; never substitute unannounced self-review or a solo fallback.

| Binding | Fill from |
|---|---|
| Scope | decomposition or composition |
| Exact subject SHA | committed subject being reviewed |
| Product plan and implementation guide | canonical paths and current SHA-256 digests |
| Independent reviewer and native root | observed peer/allocation binding |
| Requested harness, model and optional effort | resolved reviewer role and field provenance |
| Actual observed configuration | native session/PID/argv/environment evidence; unsupported facts are gaps |
| Report destination | PM-designated writable review artifact path |
| Relevant evidence | actual checks, composition receipt and complete scoped acceptance links |

## Review contract

Read product intent and the separate implementation guide. For decomposition, assess architectural independence, injected interfaces/fakes, exact ownership, dependency waves, AC-to-capability coverage and explicit composition. File count or a structural checker is not independent architectural judgement. Guide review does not require future implementation code.

For composition, inspect the exact verified artifact SHA, real entrypoint/wiring, applicable check outputs, failure cases and remaining human judgement. Unit green is not assembled behavior proof; import-only `integration_sha` is not verified `artifact_sha`. Confirm that `pressure` links name the actual selected instrument and `proven_by` links name observed execution evidence, not planned commands.

For a queued coder release, verify that the PM accepted a distinct post-release `AckReceipt` through `harness builder ack <plan> --receipt <path>` before import. Pre-work ID is `ack-<unit_id>-<full-current-source-sha>` with `packet.nonce`; confirmation ID adds `-release` after the full SHA and uses the retained `release.message_id`, not an assumed packet nonce. Repeated pre-work ack or queued transport alone is not confirmation. The peer must have refreshed native packet/canary/runtime observations after seeing the exact release and returned the new path/digest; legitimate descendant/dirty work is not a reason to demand pristine-source acknowledgement again.

Check actual peer observation time against the explicit 5000 ms skew bounds (release sent time minus 5000 ms through PM ingestion time plus 5000 ms), retained sent/queued facts and canonical confirmation path/digest evidence. Missing optional runtime facts remain gaps. Confirmation never issues a second grant; the worker follows its already-authorized scope after returning the receipt. This is review of coder evidence, not authority for the reviewer to manufacture that acknowledgement.

## Boundaries

Review is read-only against implementation and canonical lifecycle state. Do not edit `.the-flow-state.json`, `the-flow.json`, `the-flow.md`, product plan, guide, tasks, canonical team receipts, worker source, main or global/deployed settings. Write only the designated report/receipt proposal. Do not close your own findings by changing code. No push, merge, workspace retirement or inferred release authority.

## Return

Persist a report and a raw `ReviewReceipt` proposal with `record_type`, `id`, `recorded_at`, `scope`, `subject_sha`, `plan`, `guide`, `reviewer_id`, `requested`, `observed`, `verdict`, `report`, `findings`. Plan/guide/report fields are path/SHA-256 bindings. Verdict is `approved|changes-requested|blocked`; findings include stable ID, severity, description, disposition and evidence where available.

Name what actually ran and what remains unproven. Requested model configuration is not provider attestation. Do not manufacture a successful review when no independent reviewer executed it. Send report/receipt pointers to the PM; only the PM records the canonical review and advances the existing flow.
