# Reviewer packet template

**You own:** `<PM-designated report and receipt-proposal paths only>`.

**You may read:** `<product plan, implementation guide, exact subject tree and evidence paths>`.

**Your job:** independently review `<decomposition or composition>` against `<exact subject SHA>` and the requested review role.

**Done means:** a persisted report and bound `ReviewReceipt` proposal identifying observable findings, dispositions, checks actually inspected and remaining gaps.

Fill the map and bindings before delivering this briefing. It is not an issued review receipt or proof that a reviewer ran. Receiving the work packet starts this review without a separate release. Missing requested independent/cross-model capability remains unfulfilled; never substitute unannounced self-review or a solo fallback.

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

If this assignment has an issued canonical packet, include its actual path/SHA-256 after the map. Optional `harness builder self-check <packet> --sha256 <digest>` compares packet bytes, checkout root and source SHA with warning-only corrective guidance. It changes no state and is not review evidence or an import prerequisite. Never manufacture a packet or digest for a briefing that has none.

## Review contract

Read product intent and the separate implementation guide. For decomposition, assess architectural independence, injected interfaces/fakes, the write/read maps, dependency waves, AC-to-capability coverage and explicit composition. Ownership-only guide findings are warnings, not structural invalidity or permission rules. File count or a structural checker is not independent architectural judgement. Guide review does not require future implementation code.

For composition, inspect the exact verified artifact SHA, real entrypoint/wiring, applicable check outputs, failure cases and remaining human judgement. Unit green is not assembled behavior proof; import-only `integration_sha` is not verified `artifact_sha`. Confirm that `pressure` links name the actual selected instrument and `proven_by` links name observed execution evidence, not planned commands.

For `dispatch --adopt-peer <id>`, inspect the binding to the actual existing peer, parent and workspace; original adopted ownership/authority, source ancestry, progressed HEAD and WIP must be preserved. Binding is not a spawn, replay, new work grant or provider identity attestation. For `compose --import <path> --already-integrated`, every supplied unit must have a nonempty frozen map projection equal to current committed PM HEAD, falling back to actual delivery-touched paths only when no concrete mapped paths exist. Inspect optional `integration_method`/`integration_proofs`, including original delivery SHA, scope, path count and tree digest; equality includes modes/types/Git objects and absence. Original worker SHAs must remain unchanged and changes must not be reapplied. This is scoped integration evidence, not whole-patch or product proof: `compose --verify` and independent review remain required.

When `--integration-sha` was used, inspect projection equality at the recorded historical integration point, not at the later artifact. Require sealed source → integration point → artifact ancestry, then assess the PM's later delta and its warnings. No checkout or receipt rewriting is implied.

For coder deliveries, inspect current packet/dispatch/allocation digest bindings, the actual allocated tree/branch and delivered commit, distinct peer attribution and sealed-source ancestry. Wrong checkout/commit needs the correct allocated source; altered evidence needs the original bound bytes; duplicate peers need corrected actual attribution; rewritten baselines need restored history or reviewed new seals and packets. These integrity checks belong at import, not in a startup permission exchange.

Read `composition.value.warnings` alongside the exact composed bytes and real checks. Guide, coder delivery and PM map deviations are advisory and require no out-of-map approval or justification. Keep each warning's `file`, `owning_unit` and `stage` visible; a guide-only declaration can identify `<guide:field>` with cause/fix instead of a concrete file. A self-check/on-track report or queued/delivered transport is not composition proof. Keep historical startup evidence unchanged, without replay or reclassification. Missing optional runtime facts stay gaps; requested settings are not provider attestation.

For independent inspection use `harness builder on-track <plan> [--unit <id>] [--from <ref>] [--to <ref>] [--untracked]`. It writes nothing, requires no readiness, seal, review or receipt, and exits 0; read `compared`, `warnings`, actionable `issues` and the selected `basis`/full SHAs. `compared: false` means inspection was unavailable, not clean. A named unit compares all committed touched paths (including reverted writes); no unit compares the PM endpoint delta. Default end includes tracked staged/unstaged work, explicit `--untracked` adds new paths, and explicit `--to` is committed-only. Without `--from`, PM prefers imported integration, then sealed source, then HEAD; a unit uses sealed source or HEAD. Malformed existing basis evidence is an issue, not fallback.

## Boundaries

Review is read-only against implementation and canonical lifecycle state. Do not edit `.the-flow-state.json`, `the-flow.json`, `the-flow.md`, product plan, guide, tasks, canonical team receipts, worker source, main or global/deployed settings. Write only the designated report/receipt proposal. Do not close your own findings by changing code. No push, merge or workspace retirement.

## Return

Persist a report and a raw `ReviewReceipt` proposal with `record_type`, `id`, `recorded_at`, `scope`, `subject_sha`, `plan`, `guide`, `reviewer_id`, `requested`, `observed`, `verdict`, `report`, `findings`. Plan/guide/report fields are path/SHA-256 bindings. Verdict is `approved|changes-requested|blocked`; findings include stable ID, severity, description, disposition and evidence where available.

Name what actually ran and what remains unproven. Requested model configuration is not provider attestation. Do not manufacture a successful review when no independent reviewer executed it. Send report/receipt pointers to the PM; only the PM records the canonical review and advances the existing flow.
