# Coder packet template

**You own:** `<complete assigned source/test paths>`.

**You may read:** `<contract/dependency paths and their owners>`.

**Your job:** `<unit responsibility and frozen interface; dependencies and wave>`.

**Done means:** `<observable acceptance criteria, proof commands and required committed delivery>`.

Fill these four lines from the reviewed unit, including every scoped supplement. These write/read maps are guidance, not source-access restrictions. Reuse shared contracts and injected adapters/fakes to keep units independently runnable; coordinate interface changes that break consumers. This is an editable briefing, not an issued `builder/work-packet` record. The PM fills the bindings below from the canonical dispatch; never invent identities, hashes, ownership or runtime observations.

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

## Check the map while working

```bash
harness builder on-track <plan> [--unit <id>] [--from <ref>] [--to <ref>] [--untracked]
```

Use `--unit <id>` for this unit's map; omit it for all PM maps. No readiness, seal, review or receipt prerequisite; no writes; exit 0. Show `compared`, selected `basis`, measured full `from`/`to` SHAs, `includes_worktree`, `includes_untracked`, `warnings` and `issues`. Unavailable comparison returns `compared: false` with actionable issues, not a refusal.

Default comparison includes committed work through HEAD plus tracked staged/unstaged work; add `--untracked` for new paths. Explicit `--to` is committed-only, excluding all current work even with `--untracked`. `--from` chooses the basis; otherwise a unit uses sealed source or HEAD, while PM prefers imported `integration_sha`, then sealed source, then HEAD. Malformed existing basis evidence is an issue, not silent fallback. Unit history includes reverted writes; PM uses the endpoint delta.

Both coder and PM map deviations warn, with `file`, `owning_unit` and `stage` visible for independent review. Out-of-map work needs no approval or justification. This is the same comparison used automatically, not product proof or a second policy engine.

## Boundaries

Keep canonical plan/guide/task/receipt and flow mutations with the PM; never forge or rewrite immutable evidence. Advisory source maps do not authorize changes on main, unrelated workspaces, global/deployed skills or settings, push/PR/merge or destructive actions: obtain the user's authorization. The PM states validation ownership explicitly; do not run project-wide checks or generators while independent units are still in flight. Coordinate frozen-interface changes that affect consumers.

## Return

Commit the actual task changes through the repository's scoped commit surface, preserving unrelated work. Return `UnitDelivery`: `unit_id`, `peer_id`, `workspace`, `commit_sha`, `packet_sha256`, `baseline_sha`. Attach exact exports, changed paths, authored-versus-executed checks, complete acceptance dispositions, output/evidence pointers, risks, friction and one encodable improvement. Preserve attribution and ownership warnings. Import is not composition proof; the PM wires and verifies the committed composition.
