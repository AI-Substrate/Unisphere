# `harness boot` — agent briefing

## What this verb computes

Run `harness boot --json` for product readiness: checks once, then real `unisphere-proof composition`, `sdk-consumer`, `installed-cli`, `collection` and `native` modes using the Cargo executable observed by checks. It starts no services; failures preserve child evidence and stop the chain.

- Missing checks: degraded, exit 0, ready false.
- Unconfigured checks: unconfigured, exit 2.
- Failed/timed-out checks or smoke: error, exit 1, with diagnostics/remediation.
- Invalid checks JSON: error; a non-ok or wrong-command envelope is not readiness.
- All real checks and smoke succeed: ok, exit 0, ready true, scope `configuration-and-native-session-projections`.

## Your role

Read the actual toolchain and each proof result. Passing boot proves the documented configuration/native projections and their bounded revision scenarios, not lossless telemetry, complete sessions, persistent history or every future client dialect. Preserve exact source identities and failure evidence; no fabricated fields or approval.

## Watch out for

Full smoke requires real SDK/CLI/app crates. An absent target fails instead of substituting a placeholder. External Cargo builds can fetch declared registry dependencies; installed products run with isolated HOME/config and empty PATH. Permission-denied smoke requires an unprivileged POSIX user. Automated sealed behavior does not replace independent core/SDK no-network and ambient-read source review. Real TTY detection is outside the captured/explicit-mode proof. `harness doctor` checks extension/machine wiring separately.
