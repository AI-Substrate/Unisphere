# `harness boot` — agent briefing

## What this verb computes

Run `harness boot --json` for product readiness: checks once, then real `unisphere-proof composition`, `sdk-consumer`, `installed-cli`, `collection`, `native` and `git-notes` modes using the Cargo executable observed by checks. It starts no services; failures preserve child evidence and stop the chain.

- Missing checks: degraded, exit 0, ready false.
- Unconfigured checks: unconfigured, exit 2.
- Failed/timed-out checks or smoke: error, exit 1, with diagnostics/remediation.
- Invalid checks JSON: error; a non-ok or wrong-command envelope is not readiness.
- All real checks and smoke succeed: ok, exit 0, ready true, scope `configuration-native-sessions-and-git-notes`.

## Your role

Read the actual toolchain and each proof result. Passing boot proves documented configuration/native projections, bounded revision scenarios and local Git Notes attribution through external SDK/installed CLI with Git AI unavailable. It does not prove lossless telemetry, complete conversations, persistent history, network-denial traces or every future dialect. Preserve exact source identities and failure evidence.

## Watch out for

Full smoke requires real SDK/CLI/app crates. An absent target fails instead of substituting a placeholder. External Cargo builds can fetch declared registry dependencies; installed products run with isolated HOME/config and empty PATH. Permission-denied smoke requires an unprivileged POSIX user. Automated sealed behavior does not replace independent core/SDK no-network and ambient-read source review. Real TTY detection is outside the captured/explicit-mode proof. `harness doctor` checks extension/machine wiring separately.
