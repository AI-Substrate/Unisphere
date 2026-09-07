# Harness maturity assessment — the canonical L0–L4 ladder

This is the **one canonical home** for the engineering-harness maturity ladder. It lives here, not inline in the boot verb, so the ladder sits in exactly one place; the boot verb and any other reader **link** to it rather than restating the table.

The ladder is the **nucleus / self-improving** ladder. A reader reports the level that is *actually working* (not aspirational), reading the current snapshot from the governance doc's maturity section (see [`governance-doc.md`](./governance-doc.md)). The governance doc holds only the **current** snapshot; the *trajectory* across improvements lives in the `harness-change` record ledger.

---

## The L0–L4 maturity ladder

| Level | Meaning |
|-------|---------|
| L0: No harness | Commands live in tribal knowledge, scattered docs, or ad-hoc scripts |
| L1: Front door | Governance doc, harness/, CLI skeleton, AGENTS.md pointer exist; commands may be unconfigured |
| L2: Commands encoded | Build/test/run/health are confirmed and runnable |
| L3: Improvement loop active | Friction log has entries; ≥1 has been encoded into the harness; magic-wand prompts have shipped harness changes |
| L4: Self-improving | The harness regularly produces improvements during normal work; new agents onboard without human help; proof-level ceilings are tracked |

---

## How to assess — which signals map to which rung

Read the deterministic signals (the same A–J catalog the router uses) and find the **highest rung whose evidence actually holds**. Assess what is *working now*, not what is intended.

| Rung | Evidence to look for | Reads signals |
|---|---|---|
| **L0** | No governance doc, no `.harness/` nucleus, no CLI front door — commands are tribal. | A, D absent |
| **L1** | A governance doc / `.harness/` / CLI skeleton / `AGENTS.md` pointer exists — the *front door* — even if commands aren't all wired. | A, D present |
| **L2** | The boot command boots cleanly and the health/build/test/run commands are confirmed runnable (not just declared). | C (boots cleanly), B (doctor healthy) |
| **L3** | The retro ledger has entries **and ≥1 has been encoded** into the harness (a `harness-change` record exists); magic-wand prompts have shipped at least one harness change. | E (loop substrate), ≥1 `harness-change` record |
| **L4** | Improvements arrive *during normal work* as a matter of course; new agents onboard with no human help; proof-level ceilings are tracked. | sustained `harness-change` record cadence; low onboarding cost |

- **Report the working level, never the aspirational one.** A repo with a governance doc but an unproven boot is **L1**, not L2 — the boot command must actually boot to claim L2.
- **The level is a reading, not a score or a gate.** It is the dashboard reading the loop drives upward; it never blocks work.

---

## Capability axis (separate from maturity)

Maturity (L0–L4 above) is **not** the same as the agent-facing **capability axis**, which describes what the Boot → Interact → Observe layer can *do*, independent of where the engineering harness sits on the ladder. The capability axis is deliberately **unnumbered** so it never collides with the L-levels (a bare "L2" always means maturity, never capability). It roughly progresses:

**no interaction** (agent writes code, human tests) → **manual boot + API** (human starts the stack, agent sends requests) → **auto boot + API** (agent starts the stack, health check, API interaction) → **full interaction + evidence** (agent boots, drives UI/CLI, captures screenshots) → **self-healing** (auto-recovery from stale processes, auth expiry).

A harness can be high on one axis and low on the other; report them separately.
