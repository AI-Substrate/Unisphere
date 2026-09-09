---
record_kind: "retro"
harness_version: "0.14.0"
branch: "builder/001-sdk-cli-foundation"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-07T08:14:52.818Z"
agent: "pij-right-kotallo"
plan_id: "001-sdk-cli-foundation"
schema_version: "1.2"
retro_id: "2026-09-07T08:14:52.818Z-pij-right-kotallo-composition"
started_at: "2026-09-07T04:52:59.172Z"
ended_at: "2026-09-07T08:16:10.873833+00:00"
summary: "Concrete harness friction and encodable improvements from Plan001; not a product progress report. Original observations and peer custody retained separately."
entries:
  - id: "DL-001"
    kind: "difficulty"
    description: "Native subprocesses lost caller identity; errors did not name the required current PIJ_SESSION_ID."
    target: "tooling"
    severity: "degrading"
    workaround: "PM DL-001/DL-002; explicit current seat environment repaired the real command, without guessing another actor."
    suggested_encoding: "Name missing caller context and exact current-seat environment repair in the command error."
    disposition: "kept"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T04:52:59.172Z"
  - id: "DL-002"
    kind: "difficulty"
    description: "Review and acknowledgement receipt shapes differ, and native_session UUID versus session-file path was unclear."
    target: "tooling"
    severity: "degrading"
    workaround: "PM DL-003 and CLI DL-001; unchanged review payload wrapped as DD, native session UUID re-observed; original refused receipts preserved."
    suggested_encoding: "Ship minimal role-specific validated receipt examples with explicit field types."
    disposition: "task"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T05:17:31.996Z"
  - id: "DL-003"
    kind: "difficulty"
    description: "A valid proof link could point to an older source run despite newer digest-bound evidence; shape validation did not detect applicability drift."
    target: "tooling"
    severity: "degrading"
    workaround: "PM DL-004; superseding execution entries bound unchanged source digests and repointed only applicable assertions."
    suggested_encoding: "Expose source-manifest applicability and stale checked-proof diagnostics without requiring pointless reruns after documentation-only commits."
    disposition: "task"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T05:27:40.639Z"
  - id: "DL-004"
    kind: "difficulty"
    description: "A baseline write fence included .gitignore while its seal set omitted it; readiness named the unit and suggested future composition instead of the missing file."
    target: "tooling"
    severity: "degrading"
    workaround: "PM DL-005; exact coverage diagnosis identified .gitignore, then the unchanged file was included through supported guide/review/seal."
    suggested_encoding: "Preflight declared baseline coverage and name exact uncovered paths before paid review."
    disposition: "task"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T05:31:51.576Z"
  - id: "DL-005"
    kind: "difficulty"
    description: "Immutable baseline receipt reuse was diagnosed only after independent review, requiring another pointer-only review round."
    target: "tooling"
    severity: "degrading"
    workaround: "PM DL-006; selected fresh receipt identity and preserved historical seals, never overwrote or relabelled them."
    suggested_encoding: "Diagnose occupied immutable receipt destinations before review and print the complete rebaseline command sequence."
    disposition: "task"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T05:42:55.053Z"
  - id: "DL-006"
    kind: "difficulty"
    description: "Dispatch required PM HEAD equal sealed source although readiness allowed later evidence commits."
    target: "tooling"
    severity: "degrading"
    workaround: "PM DL-007; upstream row45 fixed parent ancestry while retaining strict coder source binding. Live run used equality; descendant case remains upstream unit-test evidence."
    suggested_encoding: "Preserve the ancestor-accepted and rewritten-baseline-refused regressions and expose both observed HEAD and sealed source."
    disposition: "fixed-now"
    system:
      compound:
        status: encoded
        source: agent-self
        first_seen_at: "2026-09-07T05:49:52.316Z"
  - id: "DL-007"
    kind: "difficulty"
    description: "Managed-plan unit dispatch omitted the live parent AllocationRecord at the real provision/store seam."
    target: "tooling"
    severity: "degrading"
    workaround: "PM DL-008; upstream row46 resolves the existing workspace locator; actual three-clone dispatch exercised the fix after authorized activation."
    suggested_encoding: "Keep real-Git managed-worktree-to-child allocation coverage, not only a mocked provision callback."
    disposition: "fixed-now"
    system:
      compound:
        status: encoded
        source: agent-self
        first_seen_at: "2026-09-07T06:04:02.070Z"
  - id: "DL-008"
    kind: "difficulty"
    description: "Native startup .serena metadata caused pre-work acknowledgement refusal without naming the offending paths."
    target: "tooling"
    severity: "degrading"
    workaround: "Supported clone-local .git/info/exclude preserved metadata and frozen source; all three fresh native acknowledgements then passed (row47)."
    suggested_encoding: "Provision known harness-local exclusions explicitly and list actual unexpected paths on acknowledgement refusal."
    disposition: "task"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T07:10:14.376Z"
  - id: "DL-009"
    kind: "difficulty"
    description: "Release text omitted the retained transport message ID and timestamp needed by post-release confirmation."
    target: "tooling"
    severity: "degrading"
    workaround: "SDK DL-001, CLI DL-001, proof DL-001; PM supplied existing metadata, peers emitted fresh confirmations, no second grants."
    suggested_encoding: "Include release.message_id and recorded_at in the release payload to remove a mandatory metadata round trip."
    disposition: "task"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T07:19:10.330Z"
  - id: "DL-010"
    kind: "difficulty"
    description: "Post-import integration had no supported amendment for PM formatting and necessary lint corrections even though the guide assigned formatting to PM."
    target: "tooling"
    severity: "degrading"
    workaround: "PM DL-009; retained correct source and E477 evidence rather than reverting into knowingly red quality gates or fabricating closure. Upstream row48 is implementing candidate-bound exact-path amendment."
    suggested_encoding: "Add narrow digest-bound integration amendments reviewed with the composed artifact; do not restamp frozen contracts or permit blanket ownership bypass."
    disposition: "task"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T08:08:48.079Z"
  - id: "GFT-001"
    kind: "gift"
    description: "Builder rejected an approved review carrying open material findings instead of silently accepting a contradictory verdict."
    target: "tooling"
    severity: "degrading"
    workaround: "PM GFT-001; preserved original verdict and refusal, corrected real gaps and obtained consistent current review."
    suggested_encoding: "Keep semantic verdict/disposition consistency validation with a concise actionable error."
    disposition: "kept"
    system:
      compound:
        status: suggested
        source: agent-self
        first_seen_at: "2026-09-07T05:17:31.539Z"
---

# Retro — Plan001 harness friction

Original observation payloads, bucket identities and read results are retained in
`docs/plans/001-sdk-cli-foundation/assets/harness-friction-custody.json`.
Peer buckets remain untouched; the PM clears only its own bucket after this record is committed.

The CLI mutable-writer compilation error is product execution evidence, not a harness
improvement observation. It remains in the execution log and original custody payload,
not promoted into an improvement entry here.

Highest-value remaining encoding: an exact-path, candidate-bound post-import amendment
so legitimate composition work can finish without weakening frozen-source or review identity.
Next highest: include release identity/time in the native payload instead of asking peers
to reconstruct protocol metadata. Both have been reported to the owning harness team.
