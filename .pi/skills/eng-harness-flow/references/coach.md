# coach — the eng-harness-flow human-mode voice

> **The voice, in one place.** `the-flow` is a *pleasant* experience because every
> turn shows a **progress rail**, says **where we are** and **what's next**, and
> **flags anything important the user might have missed** — in a warm,
> confirming-not-nagging voice. `eng-harness-flow` adopts the same UX. The one
> adaptation: because the router is **stateless**, the rail is **recomputed from
> substrate every call** (not read from a saved journey) — but the *feel* is
> identical. The dispatch and the verb modules carry **no** narration of their own;
> all of it lives here, so the guide is one voice, not scattered prose. The routing
> signals this voice renders (`rail`/`now`/`next`/`flags`/`insight`) come from the
> `--json` envelope in [`00-routing.md`](./00-routing.md).

---

## 1. The host rail — always first, every turn

Every human-mode turn opens with a one-line rail, then a blank line, then the narration. The rail speaks `the-flow`'s exact glyph language — **one visual vocabulary across both guides**: `◆` done · `◐` current · `◇` not yet, joined by `─` into a track. The loop's terminal is `↺`: engineering never "completes", it cycles — loop pips are **per-pass** and reset when the loop re-enters Boot. Use text-presentation `⚙` (never the `⚙️` emoji — double-width glyphs wreck the rail's spacing).

**Engineering zone** (adoption holds — the adoption segment disappears entirely; never render a completed adoption bar):

```
[eng-harness-flow] ⚙ ◆─◐─◇─◇─◇ ↺  boot · [backpressure] · observe · retro · improve

 now  · post-spec — running the backpressure survey (boot ✓ this pass)
 next · ▸ /plan-3   architect — consumes backpressure-coverage.md
```

The five loop pips are Boot · Backpressure · Observe · Retro · Improve, in that order, and the legend rides **on the rail line itself**: two spaces after the pips, the stage names in pip order joined by ` · `, the **current** one wrapped in `[…]`. Brackets follow the `◐`; on a settled rail bracket the next stage up. Same rule as `the-flow`'s rail.

**Mid-adoption** (the only time the 🧰 gate appears — two segments joined by `→`, loop still empty):

```
[eng-harness-flow] 🧰 ◆─◆─◐─◇─◇ → ⚙ ◇─◇─◇─◇─◇ ↺  install · scout · [governance] · inject · boot

 now  · adoption gate — governance missing (S2); install ✓, scout ✓
 next · ▸ <the governance step>   (routes you there)
```

Adoption pips = S0 install · S1 scout · S2 governance · S3 inject · S4 boot. The moment S0+S2+S4 hold, drop the 🧰 segment for good. The legend names the **active segment's** steps — adoption rungs while the gate is open, loop stages once in the engineering zone.

- **Stateless rail**: the fill is *derived from signals each call* (which adoption rungs hold; where in the work) — never persisted. Frame it once, early, as *an at-a-glance map, not a saved journey*.
- **Render the whole rail block as a fenced code block — always.** The rail line(s), any anchored companion line, and the `now`/`next` groups are ONE ``` fence (no language tag). Outside a fence markdown collapses leading spaces — and **never** fake alignment with `&nbsp;` or any HTML entity (terminals print them literally). Real spaces inside the fence are the only alignment tool.
- **Status line** under the rail: ` now  · <current>` / ` next · <what follows>`, aligned. When `next` has ≥2 options, stack them with `▸` (recommended first), exactly like `the-flow`.

## 1a. The unified rail — when `the-flow` is also live (chores on its rail)

Before rendering a solo rail, probe for an active SDD flow: a `docs/plans/*/the-flow.json` whose `nav.now` resolves to a mid-spine node (plan 030 — the-flow's position lives in `nav`, **not** a `.the-flow-state.json`, which is retired). If one exists, the loop does **not** draw its own bar. Instead its four fire hooks ride **as chores on the-flow's own rail** (plan 032 — *"so the main flow tracks them for us too and we don't miss things"*). This is the dogfood: render the-flow's rail straight from the CLI —

```
harness flow rail --path docs/plans/<ord>-<slug>/the-flow.json --chores show
```

— which already prints the chore square pips (`□` todo · `■` done · `▨` skipped) inline beside the spine diamonds. Show that line verbatim (real spaces only — never `&nbsp;`), then let **each flow speak with its own voice**:

```
[the-flow] ◆─◆─[ ◐ ]─◇─◇  research · plan · [ □ pre-coding · build ] · □ post-coding · review · □ post-flight

 the-flow
  now  · plan READY + validated (Simple) — mid-build
  next · ▸ /the-flow 6 implement — consumes the plan

 ⚙ engineering harness  (chores on the-flow's rail)
  now  · pre-coding chore is todo (□) — run `/eng-harness-flow --hook pre-coding`
  next · running it flips □→■ on the next rail render (AC-13 — nothing gets missed)
```

- **The rail is the-flow's, not a second bar.** Coexistence = chores injected into `the-flow.json` (the four fire hooks: `pre-flight`/`pre-coding`/`post-coding`/`post-flight`, each `run /eng-harness-flow --hook <hook>`). The chore pips ARE the harness loop's visible presence — no separate `└─ ⚙ … ↺` anchored line. The standalone `⚙` loop rail (§ 1) is only for when **no** the-flow is active.
- **Render from the CLI, never hand-drawn.** Both the solo loop rail (`harness flow rail --slug loop`) and this unified rail (`--path …/the-flow.json --chores show`) come from `harness flow rail` — the dogfood. Hand-drawn glyphs are only an illustration here.
- **Two flows, two voices — never merged, each under its own header.** ` the-flow` speaks SDD position (where the plan is, the next command); ` ⚙ engineering harness  (chores on the-flow's rail)` speaks the chores — which fire hooks are `todo`/`done`, and that running one flips its pip on the next render. Each group's `now`/`next` lines are indented one space under its header.
- **Never invent the `the-flow` line**: read it from `harness flow rail`/`nav show` on `the-flow.json` + the newest artifact. Unreadable → fall back to the solo rail.
- Mid-adoption with an active `the-flow`: adoption is the live harness flow (the loop hasn't started), so there are no loop chores yet; show the-flow's rail plus a one-line ` ⚙ engineering harness` note that adoption is still underway (`🧰` segment, `(adopting)`), and route the missing rung.

## 2. The per-turn narration contract — Orient → Flag → Insight → Suggest → Invite

Every turn follows the same five beats (one decision per turn, a recommended default + an "if unsure" path):

| Beat | What it does | Example (engineering, post-spec) |
|---|---|---|
| **Orient** | one line: which zone + stage, from the rail | "Adoption's done — you're in the engineering loop, just past the spec." |
| **Flag** ⚠️ | surface must-see items the user might've missed (see § 3) — *confirming, never nagging*; **silent when clean** | "⚠️ Boot flagged `no smoke path declared` — worth knowing before we lean on it." |
| **Insight + why** | one *real* detail, tied to why the stage matters — the shape is *"Did you notice `<detail>`? That matters because `<this stage's line from the why table>`"* | "9 of 11 criteria are already provable by existing sensors — that matters because this is the whole game: moving proof from inference into the deterministic layer, where it's runnable and free to re-check forever." |
| **Suggest** | print the **one** next command in a copyable block | `/eng-harness-flow --hook pre-coding` |
| **Invite** | offer to run it; recommend the default, never force | "Want me to run it? (`yes` / run it yourself — either way I'll pick up from here.)" |

This is the same **print-then-offer** posture as `the-flow`: always show the command first (copyable anywhere), then offer to run it; **one step per turn**; **never anything irreversible without explicit go-ahead**.

## 2a. The why table — what each stage is *for*

The teach-half of the Insight beat is drawn from this fixed table (never invented), fused with one **real** detail from the routed artifact. One line max, pitched at someone meeting the loop for the first time. **Skip the teach-half when the user clearly knows the loop** — a returning operator, or a stage already taught this session — confirming, never lecturing. The detail half is still subject to the no-fabrication rule: read the artifact, quote what's there.

| Stage | Why it matters (the thesis link) |
|---|---|
| S0 · Install | the deterministic layer gets a **front door** — one discoverable place (`--help`, `doctor`) instead of diffuse scripts and tribal knowledge |
| S1 · Scout | measures the **proof ceiling** — how much of this repo can be *proven* today vs eyeballed |
| S2 · Governance | the contract that makes the layer **tangible** — what boots it, what proves it, where evidence lands |
| S3 · Inject | wires the harness into the flow you already run, so usage is **structural, not remembered** — it survives every cold agent start |
| S4 · Build boot | the first proof — the environment runs **before** any work starts |
| Boot (loop) | orientation by **evidence, not memory** — prove the system runs before you touch it |
| Backpressure | moves proof from **inference to determinism** before you build — what can the repo *prove* about this work, and what would still be eyeballed? |
| Observe | friction is **usability research** on the engineering environment — one line now, a candidate fix later |
| Retro | "what did you have to infer that the harness should have proved?" — every answer names a **missing command** |
| Improve | the compounding move — **improve = encode the fix**: inferred knowledge becomes a runnable part of the deterministic layer, and the next session starts smarter |

## 3. The Flag beat — "just making sure you saw this"

Distinct from the single Insight (curiosity), the Flag beat surfaces the **decision-relevant must-sees** the user can't afford to miss (safety). Rules, lifted in spirit from `the-flow`:

- **Lift, never derive** — quote the routed artifact's own alarm fields (a degraded `doctor` reason, a Critical/High harnessability gap, an `UNAVAILABLE`/failed/SLOW boot, ABSENT/BUILDABLE sensors, a recommended Phase 0, pending retro entries). Never invented.
- **Cap it** — a few max; this is a highlight, not a dump.
- **Silent when clean** — nothing flagged → one line ("nothing flagged — clean") or skip the beat entirely. No manufactured alarms.
- **Never a gate** — "just making sure you saw" — the user acts on it or waves past. It never blocks the next step.

| Stage | Scan for / flag (quote any hits) |
|---|---|
| Install / `doctor` | degraded or failed `doctor` reasons (read the JSON envelope, not prose) |
| Unused harness | harness CLI present (signal A) but **no boot this session** / no `harness instructions` self-brief yet — flag it: "harness is installed but we haven't booted or briefed from it yet; that's how it goes unused." |
| Scout (harnessability) | Critical/High gaps — low proof ceiling, missing back-pressure surfaces, external-dependency exposure |
| Governance | doc absent, or stamped-but-empty (no boot command yet) — boot reports `UNAVAILABLE` until `harness init` runs and S4 builds boot |
| Inject | no injection point recorded yet (so the parent flow won't know where to call back) |
| Build + run boot | `UNAVAILABLE`, a **failed/SLOW** boot, or a signal-readiness dimension reported "not declared" |
| Backpressure | **ABSENT / BUILDABLE** sensors (the eyeball-gaps); a recommended **Phase 0** |
| Retro drain | the save prompt the user just saw (keep all · pick · skip, or take them further); N notes pending |
| Retro harvest | clustered/stale friction across the plan; unencoded magic-wands |
| Ambiguous | the candidate plans found (so the user can pick) |

> **Backpressure → defend the proof (optional).** When the survey flags `ABSENT`/`BUILDABLE` sensors (thin coverage), offer `/grill-agent-done` before architect — a standalone peer skill (`skills/grill-agent-done/`, **not** a routed stage) that defends the definition of done one claim at a time, lining each against the right proof grade. The deep-interrogation companion the survey deliberately isn't. Never blocks; offer once, skip freely.

> **Retro drain → plain words, never codes.** When you offer the drain, describe the choice in plain language — *"I jotted a few notes on what slowed us down; want me to save them so they're not lost? You can keep all, pick which to keep, or skip."* **Never** surface the internal `[s/t/p/e/d/a]` / `[r/w/s]` letter codes in narration, and **never guess what they expand to** — they are *keep-all / pick / skip*, with *tasks / plan / diffs* to take them further (the codes are emphatically **not** "trim"/"promote"; parroting them is how the prompt became arcane). The codes are an implementation detail; the user sees plain choices and a recommended default. Harvest marks are the same: "done" / "won't-fix" / "stale", never `[r/w/s]`.

## 4. Tone — make it pleasant

- **Warm and confirming**, never bureaucratic. "Nice — boot's green, you're ready to code" beats "S4 precondition satisfied."
- **One decision per turn.** Never dump the whole tree; surface the single next move + a couple of alternates.
- **Celebrate the bridge.** When adoption finishes and boot first runs, say so — "🎉 boot's working — that's the harness alive; let's try it on real work." (the "shiny new harness" moment).
- **Never nag — but always offer, out loud.** Offer a skipped optional at most once per call; the user may wave it past freely. "Never nag" governs *tone and repetition*, not whether you surface it at all — silently dropping the boot, the survey, or the end-of-plan encode offer is the failure the loop exists to prevent, not politeness. Flags stay "just making sure you saw," never blockers.
