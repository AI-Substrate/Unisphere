---
record_kind: "retro"
harness_version: "0.14.0"
branch: "builder/028-prep-canonical-tables"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-29T10:10:22Z"
agent: "pij-native-tick"
plan_id: "028-prep-canonical-tables"
schema_version: "1.2"
retro_id: "2026-09-29T10:10:22Z-pij-native-tick-postflight"
started_at: "2026-09-29T10:05:41.723Z"
ended_at: "2026-09-29T10:10:22Z"
summary: "Plan 028 post-flight drain (PM): two Builder closeout frictions found while archiving - compose replay drops git-ai notes, and the close->commit order breaks the post-flight exit gate. Both kept for the operator's upstream-issue decision."
entries:
  - id: DL-001
    kind: difficulty
    description: "harness builder compose --import replays coder commits as new SHAs without their refs/notes/ai attribution: on the 028 branch 12 replayed lane commits carry no git-ai note while the originals in the coder clones do; attribution survives only if the clones' refs are preserved (builder close does, if every clone allocation is passed)."
    target: harness-itself
    severity: degrading
    workaround: "Passed every coder clone allocation to builder close so their notes refs are preserved"
    suggested_encoding: "compose --import should carry refs/notes/ai onto replayed SHAs (git notes copy) or record the original->replayed mapping; file upstream."
    fp: "bd426deb98f0"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T10:05:41.723Z"
  - id: DL-002
    kind: difficulty
    description: "harness builder advance post-flight -> ship refused E476 'Source evidence changed after preservation' because committing close's archive move changed the working-tree diff close had preserved; the close -> commit order the skill implies is incompatible with the exit gate. Re-ran close on the archived plan (fresh preservation generation) and advanced before committing."
    target: harness-itself
    severity: degrading
    workaround: "Re-ran close as an evidence refresh, advanced, then committed"
    suggested_encoding: "Let close verify/advance against the committed archive (or have close commit the move itself) so post-flight exit does not require a refresh generation; file upstream."
    fp: "2ac06e2525ef"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T10:10:11.016Z"
system:
  compound:
    bubble_action: "all-save"
---

# Retro - Plan 028 post-flight (PM pij-native-tick)

Both entries are harness-itself; held with the phase retros' Builder entries for the operator's upstream-issue decision.
