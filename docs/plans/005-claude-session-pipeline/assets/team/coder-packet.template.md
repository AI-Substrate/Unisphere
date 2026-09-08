# Coder packet template

**You own:** `<complete assigned source/test paths>`.

**You may read:** `<contract/dependency paths and their owners>`.

**Your job:** `<unit responsibility and frozen interface; dependencies and wave>`.

**Done means:** `<observable acceptance criteria, proof commands and required committed delivery>`.

Fill these four lines from the reviewed unit, including every scoped supplement. Reuse shared contracts and injected adapters/fakes; do not import a same-wave sibling implementation or silently renegotiate interfaces. This is an editable briefing, not an issued `builder/packet` record. The PM fills the bindings below from the canonical dispatch; never invent identities, hashes, ownership or runtime observations.

| Binding | Fill from |
|---|---|
| Unit and responsibility | reviewed implementation guide unit |
| Native peer and workspace root | observed allocation/runtime binding |
| Parent | governing PM identity |
| Source baseline SHA | current packet `source_sha`, bound to the sealed baseline |
| Canonical packet path and SHA-256 | actual dispatch result |
| Plan, guide and allocation | packet's path/digest bindings |
| Requested harness, model and optional effort | resolved role plus field provenance |

## Start the work

Receiving the actual work packet means carry out its unit, without a separate release. Read the packet and the named contract paths. For optional orientation from your actual checkout, use the packet pointer and digest supplied by dispatch:

```bash
harness builder self-check <packet> --sha256 <digest>
```

This one read-only check compares packet bytes, repository root and HEAD/source SHA. Missing or mismatched observations warn with a cause and corrective `next_action`; they do not refuse work, write state or supply an import permission. No receipt response is required. Historical startup records remain unchanged; never replay completed work or reclassify old receipts.

Import still verifies actual tree/branch/commit, exact current packet/dispatch/allocation digests, distinct peer attribution and sealed-source ancestry. Wrong checkout or commit: deliver from the allocated branch with its actual SHA. Mismatched evidence: recover the original bound bytes rather than altering them to fit. Duplicate peer: correct attribution to the actual distinct dispatched workers. Rewritten baseline: restore its history or ask the PM for reviewed, newly sealed contracts and new packets.

## Boundaries

Write only the declared source/test fence. Do not edit `.the-flow-state.json`, `the-flow.json`, `the-flow.md`, canonical plan/guide/task/receipt state, other workers' paths, main, global/deployed skills or settings. No push/PR/merge or unrelated deletion. The PM states validation ownership explicitly; do not run project-wide checks or generators while independent units are still in flight. Ask the PM before changing frozen interfaces.

## Return

Commit only the declared source/test paths through the repository's scoped commit surface. Return `UnitDelivery`: `unit_id`, `peer_id`, `workspace`, `commit_sha`, `packet_sha256`, `baseline_sha`. Attach exact exports, changed paths, authored-versus-executed checks, complete acceptance dispositions, output/evidence pointers, risks, friction and one encodable improvement. Preserve attribution warnings. Import is not composition proof; the PM wires and verifies the committed composition.
