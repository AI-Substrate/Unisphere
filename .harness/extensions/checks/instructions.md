# `harness checks` — agent briefing

## What this verb reports

Run `harness checks --json` before considering product work done. It currently returns `status: unconfigured`, exit 2, and a concrete `next_action`: Unisphere has no canonical product build/test lane yet. No product tests, lint, or compilation are run.

## Your role

Treat this as missing proof, not a passing gate and not a tool crash. Once product implementation establishes a supported validation command, replace the explicit unconfigured result in `extension.ts` with a bounded `ctx.exec` call wrapping that command. Do not infer a toolchain from the Rust-oriented ignore file, or invent an unrelated check just to turn the status green.

Keep this gate separate from service startup. `harness boot` composes it, so quality commands belong here once rather than being duplicated in callers. A failed real command must return an error and nonzero exit.

## Proof boundary

A clean `harness doctor` extension/convention result proves harness wiring only. It does not prove telemetry ingestion, normalized output, or persistence. Update this briefing and `.harness/engineering-harness.md` when the canonical product lane is added.
