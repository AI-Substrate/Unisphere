---
record_kind: "retro"
harness_version: "0.14.0"
branch: "builder/028-prep-canonical-tables"
repo: "https://github.com/AI-Substrate/Unisphere.git"
created_at: "2026-09-29T07:03:36.143Z"
agent: "pij-native-tick"
plan_id: "028-prep-canonical-tables"
schema_version: "1.2"
retro_id: "2026-09-29T07:03:57Z-pij-native-tick-p1drain"
started_at: "2026-09-29T04:30:30.165Z"
ended_at: "2026-09-29T07:03:57Z"
summary: "Plan 028 phase-1 drain (PM): guide/seal/dispatch/compose friction with the Builder lifecycle, tooling and machine load; one win for the deterministic prep proofs. All entries kept; the two harness-itself entries are offered as upstream issues."
entries:
  - id: DL-001
    kind: difficulty
    description: "harness builder compose imports every coder unit of a guide exactly once on one frozen baseline SHA (composition-service.ts compositionRoster/import roster checks), so a multi-phase plan whose phase-2 coders must start from the phase-1 composition cannot declare both phases' coder units in one guide; Plan 028 guide v1 carries phase 1 only and phase 2 becomes guide v2 re-sealed on the phase-1 composed SHA. Also guide --check reports only a warning for capabilities owned by a not-yet-declared phase."
    target: harness-itself
    severity: degrading
    workaround: "Carry phase 1 in guide v1; declare phase-2 lanes in contracts; re-seal a guide v2 on the phase-1 composed SHA."
    suggested_encoding: "Let a guide declare per-phase coder rosters (or per-wave baselines) so compose imports one phase at a time; file upstream on AI-Substrate/harness-engineering."
    fp: "214cc144fa98"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T04:30:30.165Z"
  - id: DL-002
    kind: difficulty
    description: "Plan worktree unisphere-prep has no package.json/node_modules, so the builder recipe's local node_modules/.bin/ddocs is absent; used the main checkout's ddocs by absolute path."
    target: tooling
    severity: annoying
    workaround: "Used /Users/jordanknight/substrate/unisphere/unishpere-main/node_modules/.bin/ddocs by absolute path."
    suggested_encoding: "Plan workspaces created by harness builder new should carry package.json (or run npm ci) so node_modules/.bin/ddocs exists."
    fp: "79581d59c2fe"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T04:30:35.159Z"
  - id: DL-003
    kind: difficulty
    description: "harness builder dispatch spawns coders with 'pij-rs spawn --bin omp'; current pij-rs refuses a bare --bin ('absolute executable-path override, not a harness selector'), so every new dispatch fails E473 after provisioning the clone. PM workaround for Plan 028: PATH shim /tmp/plan028/pijshim/pij-rs that only resolves --bin omp to /Users/jordanknight/.npm-global/bin/omp. Fix belongs in harness dispatch-service.ts (omit --bin or pass an absolute path)."
    target: harness-itself
    severity: blocking
    workaround: "PATH shim /tmp/plan028/pijshim/pij-rs resolving --bin omp to its absolute path; spawn, packet and receipts otherwise canonical."
    suggested_encoding: "dispatch-service.ts: omit --bin (use --harness omp) or pass an absolute omp path; file upstream on AI-Substrate/harness-engineering."
    fp: "8a1c3153d9a2"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T04:58:55.339Z"
  - id: DL-004
    kind: difficulty
    description: "Machine load average 128-193 during Plan 028 composition (other agents/builds): cargo builds take ~10 min and crates/app/tests/git_query.rs failed 3 tests once under load, then passed alone. Timing-based real-workload numbers (AC-000c) must record load; measure.py now captures os.getloadavg()."
    target: infra
    severity: degrading
    workaround: "Recorded load averages with every timing; reran the flaky git_query test alone (passed)."
    suggested_encoding: "measure.py records os.getloadavg(); consider a harness doctor load advisory before timing-sensitive proofs."
    fp: "e49f36f20cce"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T06:00:43.456Z"
  - id: WIN-001
    kind: win
    description: "Deterministic sensors carried phase 1: unisphere-proof prep (built/installed CLI + external SDK consumer) and the numbers-only parity/live/measure scripts turned every AC-0001/0002/0004 claim into a rerunnable proof; the independent reviewer reproduced vd-0008 in its own clone."
    fp: "7e6c2e0f184b"
    disposition: kept
    system:
      compound:
        status: open
        source: agent-self
        first_seen_at: "2026-09-29T07:03:35.684Z"
system:
  compound:
    bubble_action: "all-save"
---

# Retro — Plan 028 phase 1 (PM pij-native-tick)

Highest-value encoding: DL-003 (dispatch passes a bare `--bin omp` that current pij-rs refuses) — every Builder dispatch fails until it is fixed upstream; a one-line change in dispatch-service.ts.
