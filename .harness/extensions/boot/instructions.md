# `harness boot` — agent briefing

## What this verb computes

Run `harness boot --json` for foundation readiness. It invokes `harness checks --json` once, accepts only its valid `command: checks`, `status: ok` envelope, then runs the real `unisphere-proof composition`, `sdk-consumer` and `installed-cli` commands. It starts no services. Each child is bounded; failure retains captured evidence and stops the chain.

- Missing checks: degraded, exit 0, ready false.
- Unconfigured checks: unconfigured, exit 2.
- Failed/timed-out checks or smoke: error, exit 1, with diagnostics/remediation.
- Invalid checks JSON: error; a non-ok or wrong-command envelope is not readiness.
- All real checks and smoke succeed: ok, exit 0, ready true, scope `configuration-sdk-cli-foundation`.

## Your role

Use an approved coherent Rust toolchain; the checks stage records actual versions/commit identities before running gates. Read status and ready, not exit code alone. A green foundation boot means explicit configuration, SDK use and installed CLI behavior were exercised; it never means native telemetry collection runs. Retain the actual evidence and exact subject commit in PM-owned delivery records.

## Watch out for

Full smoke requires real SDK/CLI/app crates. An absent target fails instead of substituting a placeholder. External Cargo builds can fetch declared registry dependencies; installed products run with isolated HOME/config and empty PATH. Permission-denied smoke requires an unprivileged POSIX user. Automated sealed behavior does not replace independent core/SDK no-network and ambient-read source review. Real TTY detection is outside the captured/explicit-mode proof. `harness doctor` checks extension/machine wiring separately.
