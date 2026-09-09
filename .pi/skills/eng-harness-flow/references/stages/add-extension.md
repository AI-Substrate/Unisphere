# add-extension

> Sub-skill — a harness-blind verb module. Knows only its own domain work
> (the `harness new` / `harness doctor` CLI, `.harness/extensions/`, the verb
> it authors). No sibling names, no flow position, no lifecycle-hook
> self-reference, no routing. Composition is the router's job.

**Verb**: add-extension
**Purpose**: Scaffold and validate a new `harness <verb>` extension — the **encoding move**: turning inferred knowledge (a weird boot sequence, an eyeballed check, a tribal command) into a runnable, discoverable part of the repo's deterministic layer. The CLI owns the scaffolding (`harness new`); this verb owns the **judgement** — figure out the verb, scaffold it, fill the handler, prove it loaded.
**Consumes**: already-gathered intent (a live conversation / spec / plan / workshop describing the extension) when obvious; otherwise one short clarifying question (name · what it does · TS-or-JS).
**Flags**: driven via `harness new <name> [--wrap "<command>"] [--js] [--force]`.
**Produces**: a loadable extension package at `.harness/extensions/<name>/` (`extension.ts` or `.js` + a starter `instructions.md`); a best-effort `harness-change` record.
**Side effects**: writes the extension package; optionally writes a `harness-change` record.

> **Stay thin.** If the wanted extension is obvious from context, just build it. If it is ambiguous, ask one short question. Do **not** build an elaborate context-ranking system — obvious → use it, ambiguous → ask.

---

## Procedure

### Inputs you need (and where to get them)

A verb needs three things. Gather them in this order, stopping as soon as it's clear:

1. **From the live conversation / a spec / plan / workshop** — if the user (or an in-flight spec-driven flow) already described the extension, use that. e.g. a spec that says "add a `ci-smoke` verb that runs `just ci-smoke`" gives you the name *and* the wrapped command.
2. **Ask only what's still missing.** Typically just:
   - **name** — the verb (`harness <name>`); lowercase, hyphenated.
   - **what it does** — either *wrap a real repo command* (most common — the harness's "wrap, don't rebuild" principle) or *custom logic*.
   - (optional) **TypeScript or plain JS** — default TypeScript.

If everything is obvious, skip the questions entirely.

### 1. Scaffold with `harness new`

Run the CLI scaffolder (it writes a valid, loadable stub — never start from a blank file):

```bash
# wrap a real command (most common):
harness new <name> --wrap "<command>"      # e.g. harness new ci-smoke --wrap "just ci-smoke"

# minimal custom-logic stub:
harness new <name>

# plain JavaScript instead of TypeScript:
harness new <name> --js
```

The extension lands as a little package at `.harness/extensions/<name>/` — `extension.ts` (or `.js`) plus a starter `instructions.md` (the briefing for the calling agent; author it as part of this verb, then check it with `harness instructions <name>`). Confirm the `ok` envelope and note `data.path` + `data.instructionsPath`. The stub already loads and returns `unconfigured` ("not built yet") until you fill it.

> `harness` is the npx-installed core. If it isn't on PATH, the repo installed it via `npm install github:AI-Substrate/harness-engineering` (or a local path); use `npx harness …` if needed.

### 2. Fill the handler from the gathered intent

Open the scaffolded file and implement `run(ctx)`:

- **Wrap variant** — the `ctx.exec(...)` body is already wired to the command you passed; adjust the success/error data and `next_action` if needed. Often it's already correct.
- **Custom variant** — replace the `ctx.unconfigured(...)` line with real logic. Use the `ctx` helpers: `ctx.ok(data)`, `ctx.degraded(data, next_action)`, `ctx.error(code, message, { next_action })`, `ctx.unconfigured(next_action)`. Read inputs from `ctx.options`/`ctx.args`; reach the filesystem/git/env via `ctx.fs`/`ctx.git`/`ctx.env` (never import `node:*` directly).

See [authoring-verbs.md](https://github.com/AI-Substrate/harness-engineering/blob/main/harness/cli/docs/authoring-verbs.md) (and [extend-the-harness.md](https://github.com/AI-Substrate/harness-engineering/blob/main/docs/how/extend-the-harness.md)) for the full contract.

### 3. Verify — show the proof

Never claim success without checking. Run and surface the output:

```bash
harness doctor          # the new extension must show `loaded` (not failed/conflict)
harness help            # the verb must appear in the list
harness <name> --help   # usage renders
harness <name>          # invoke it — ok (filled) or unconfigured exit 2 (still a stub)
```

Report to the user: the path created, that `doctor` shows it loaded, and the result of invoking it.

### 4. Record the change (best-effort)

Once the extension verifies, log it as a `harness-change` record so the changelog reflects what was added:

```bash
harness record harness-change --slug <name>
# change_type: new-command (or sensor) · target: the verb/recipe added · resolves: why it was added
```

Optional and non-blocking — if the harness isn't configured the command exits `unconfigured` (exit 2) and nothing else changes. Skip it for throwaway or experimental extensions.

### Guardrails

- **Reserved names**: `help`, `doctor`, `new` are core commands — `harness new` will reject them (E151). Pick another name.
- **`--wrap` is for simple `cmd arg arg` commands** (v1). For anything with quotes, pipes, or shell operators, scaffold without `--wrap` and write the `ctx.exec(...)` calls by hand.
- **Don't overwrite by accident**: `harness new` refuses an existing file unless `--force`.
- **Trust model**: extensions run with full Node privileges (like an ESLint plugin). Only add extensions you'd run anyway.
- **Composition is via `ctx.exec('harness', [...])`**: a verb can call another verb by shelling the CLI — there is no in-process verb-call API. The canonical example is the nucleus pair `checks` (the mandated lint/test/typecheck gate) and `boot`, where `boot` readies the system and then runs `harness checks` as a stage. When authoring `boot`, prefer composing `harness checks` over re-listing its gates; if `checks` is absent, have `boot` degrade with a warning that points the user at `harness new checks`.

## Exit

Print the output-contract summary (✅: what was produced, where, key fields). Then STOP — do not name a next step or route onward. Routing is the router's job.
