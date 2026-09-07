# `harness boot` — agent briefing

## What this verb runs

Run `harness boot --json` at session start. It invokes `harness checks --json` with a 120-second deadline. It starts no services and changes no product state.

- Missing checks extension: `degraded`, exit 0, `ready: false`, with instructions to create the quality gate.
- Checks unconfigured: `unconfigured`, exit 2, with the missing product build/test and readiness lane named.
- Checks process failure: `error`, exit 1, with captured stdout/stderr and the child exit code.
- Checks completes: still `degraded`, exit 0, `ready: false`, with the child envelope in `data.checks`; no product readiness command exists yet.

## Your role

Do not report that Unisphere runs merely because this command executed. The repository has not implemented the telemetry collector or a supported startup/smoke lane. `unconfigured` is the honest result during system setup.

When product work supplies the canonical readiness command, wrap it here and retain composition through `harness checks`; do not repeat quality commands. Prove a real telemetry input-to-observable-output scenario before changing readiness to success. Update `.harness/engineering-harness.md` and this briefing together.

## Watch out for

Exit 0 also represents `degraded`. Read `status`, `data.ready`, and `next_action`, not just the process exit code. Missing global harness CLI or a timed-out child is an execution error, not proof that the product failed. `harness doctor` reports extension wiring and machine attribution health separately from product readiness.
