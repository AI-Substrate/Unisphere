# Git-ai Git Notes ingestion — implementation brief

## Authority and purpose

Jordan assigned `pij-forthcoming-araminta` to implement this feature. Main allocated the worktree through `harness builder new`; no replacement worker or clone is authorised.

Users need to connect native agent activity to the commits, files and lines that actually landed. Git-ai notes provide attribution evidence even when Git AI is not installed on the reading machine. Treat those notes as an independently parsed input format and project their declared facts into Unisphere's common telemetry contract. Do not misrepresent attribution as a complete conversation, tool-timing stream or token ledger.

The canonical product intent and ten unchecked acceptance criteria are in `../plan.dd.json`. `ready` means product intent, not a reviewed guide, completed implementation or fabricated dispatch.

## Workspace and ownership

- Workspace: `/Users/jordanknight/substrate/unisphere/unisphere-git-ai-notes`
- Branch: `builder/014-git-ai-notes`
- Base: `1469998cd750d23d2420feb83cf8e93a0841b347`
- Plan: `docs/plans/014-git-ai-notes/plan.dd.json`
- Allocation: `al-014-6b87d8a2-3ae4-4954-9d96-ee5d8dac94ea`; authority receipt lives outside the worktree under main's `.git/builder/allocations/`.
- Assigned implementation owner: `pij-forthcoming-araminta`. Main is the integration/review coordinator. You own product changes and this plan's guide/tasks/execution records after this handoff; Main will not concurrently edit those files.
- Native session root remains the main checkout. EVERY write/edit must name an absolute path beneath the allocated workspace. EVERY shell/Git invocation must set `cwd` to that workspace. No relative-path edits from the default root.
- This is an explicit-path/cwd handoff, NOT native-root rebinding or formal Builder dispatch. Installed dispatch rejects linked-worktree coders; do not replace the workspace with a clone or restart/reload/reparent the seat.
- Actual exposed model: `github-copilot/gpt-6-astra`; effort is unobserved, not claimed. No runtime setting change is requested.
- Local DD tooling is provisioned using the existing repository convention: ignored `node_modules/.bin/ddocs` and package links to the already-installed DD package. Do not modify the shared installed package.

## Scope: one Unisphere-supported source

Implement explicit local repository/note selection, a read-only Git-object loader, independent Git-ai note parsing/mapping, SDK orchestration, registered CLI composition and OTLP LogsData output. Choose the smallest coherent public API/command shape after inspecting current SDK/CLI patterns; document it in the guide before code.

Keep Git-object storage access in a concrete outer adapter. Core contracts and pure mapping must not invoke Git, read the filesystem/environment, fetch network data or observe clocks. Use constructor injection and an explicit app composition root. Provenance must retain repository identity, requested note ref, pinned ref tip, target commit, note blob and relevant file/range/attestation identifiers. Do not fabricate byte offsets for Git objects or turn checkpoint IDs into OpenTelemetry span IDs.

Read both current session records and declared older prompt/human variants of the Git-ai authorship format without inventing unavailable fields. A metadata schema version alone does not imply one record shape. Native attribution keys, inclusive line ranges, null/unknown fields and content policy must remain meaningful to consumers.

## Hard exclusions and dependency boundary

Jordan explicitly clarified that retired harness telemetry is unsupported and the cutover is to Unisphere-supported ingestion:

- NO `refs/harness-telemetry` reader, segment/rollup import, migration, compatibility shim or parallel legacy path.
- NO Git AI installation requirement, executable invocation, imported implementation code, crate/library dependency, daemon, HTTP backend or private cache dependency. Standard read-only Git operations are allowed. Format research is not permission to copy Git-ai implementation.
- NO broad session exploration/query/extraction CLI. That has a separate forthcoming Builder plan and PM.
- NO source ref/note/index/worktree/config/hook mutation; no fetch, push or sync; no global Git trace2/collector/toolchain settings changes.
- NO commits on main, public pushes/PRs/merges, workspace retirement, real-data fixture publication, governance edits or other plan changes.

Existing supported native adapters continue to work. The distinction between old Git-ai note variants and retired harness telemetry is intentional: the former are source-format compatibility, the latter are expressly out of scope.

## Evidence and starting points

`git-ai-reconnaissance.json` contains structural observations and real object locators only. Do not commit real private note blobs or dereference message URLs.

Observed examples:

- Unisphere commit `d4679f44952851204e5b46dcc0f201d86802256b`, note blob `644472487cf51e8f8bfe7a7123bda097bb52bc54`: authorship/3.0.0, Git-ai 1.6.21, six file attestations and one declared session.
- Unisphere commit `4a907d6e2832f3907c6538809b4e07e7d2c7a724`: known-human map.
- Git-ai repository commit `000841e6607a4289f39ac3aa57f964c8418ebc4d`, note blob `70ea7fe03643512c109b1d5b38ad525121c451dd`: older prompt-map variant under the same schema version.

Research-only references: `/Users/jordanknight/github/git-ai/specs/git_ai_standard_v3.0.0.md`, `src/authorship/authorship_log_serialization.rs`, `src/authorship/authorship_log.rs`, `src/git/refs.rs`, `src/git/notes_api.rs`. Read them to understand the format; implement independently.

Unisphere references in your worktree: `docs/telemetry-profile.md`, `docs/sdk.md`, `docs/cli.md`, `crates/core/src/collection.rs`, `crates/core/src/snapshot.rs`, `crates/sdk/src/collection.rs`, `crates/sdk/src/snapshot.rs`, `crates/app/src/adapters.rs`, existing loader/adapter/output crates and `.harness/engineering-harness.md`.

## Proof that matters

Use synthetic temporary repositories created with ordinary Git, including real note objects. Exercise the actual external SDK and built/temporary-installed CLI, not only mocked Git output.

Prove: mixed session/prompt/human attribution; metadata versus explicit content behavior; empty notes versus invalid input; malformed/unsupported/oversized notes; stable pinned note/ref provenance; bare repositories and linked worktrees; path/argument safety; output failures; and unchanged source refs/index/worktree. Missing Git must fail clearly. Git AI must be unavailable during a successful proof, with no dependency or invocation hidden through a wrapper executable.

Control Git's external side effects, including lazy fetching and inherited repository/config environment, rather than assuming every read subcommand is hermetic. Never sum copied tracking refs as independent activity. Source/commit timestamps are not event execution times. Failed reads or truncated output are not success.

Run focused proofs first, then the existing documented quality/readiness lanes once for the composed change. Keep only regression tests defending plausible failures. Do not run tests for unrelated in-flight work. Record exact commands, cwd, exits, source SHA and observed limitations. No self-approved independent review.

## Start now

1. Read product intent/ACs and this brief; verify the explicit workspace fence.
2. Replace the generated teaching guide with a real Git Notes guide through DD tooling. The current guide seed is an unrelated text-to-HTML example: NONE of its architecture, units, checks or approval claims is authority. Do not execute or copy it into product work.
3. Populate the real guide, phase tasks and proof map using repo-local DD tooling. Follow live Builder help: it is warning-first/map-first and has no old pre/post-release acknowledgement gate. Do not resurrect stale ceremony from skills.
4. Send Main a concise guide/interface checkpoint for independent review before product mutations. Product implementation is authorised by Jordan once the real guide review boundary is satisfied; there is no further routine permission question.
5. Implement and exercise this scope; commit owned paths with `harness commit`, reading its attribution outcome. Return branch/head, SDK/CLI usage, proof paths, remaining findings and an exact changed-file list. Do not land or publish.

Start the assigned guide/task work immediately after reading this handoff. Send an acknowledgment naming the plan/workspace, clean-cutover/no-Git-AI-dependency constraints and first concrete action. No additional generic acknowledgment or permission loop is requested.
