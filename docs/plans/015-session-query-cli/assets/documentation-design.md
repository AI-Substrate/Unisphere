# Documentation is a product surface

## User intent

Jordan requires rock-solid docs and explicitly names Flowspace3's docs and implementation as the reference. A newcomer should be able to ask an installed Unisphere binary what it does, why each workflow exists, what data it will expose, and how to interpret a result before granting it access to any session store. An agent should not have to browse repository source or improvise flags to operate safely.

This is a delivery criterion for the CLI plan, not a documentation task after code completion. The command owners own the accuracy of their examples; a dedicated docs owner integrates the operating guide and checks. The PM owns consistency across both.

## What was actually inspected in Flowspace3

- Ran installed `flowspace3 0.5.0`, `docs --help`, `docs list`, and `docs get agents/search`. `docs list` returned ten topics with name, title and byte size, and an explicit next action.
- Used Flowspace3 semantic search scoped to `git:github.com/AI-Substrate/flowspace3`, code only. Followed the indexed `crates/cli/tests/docs_bundle.rs` file through `flowspace3 get`, then read the implementation in `crates/cli/src/docs.rs` and the dispatch in `crates/cli/src/main.rs`.
- The implementation uses a typed static topic registry and `include_str!` for Markdown located inside the CLI crate. This packages the docs with the binary instead of assuming the source checkout still exists.
- `docs list` exposes ordered summaries. `docs get` returns topic/title/text/related, and an unknown topic names the available choices rather than making the reader guess again.
- The bundled operating guide covers the loop, output envelopes, interpretation, common wrong turns and related pages. It is a deliberately condensed operational artifact, not a second copy of every long repository document.
- Inspected tests for real-binary command existence, operating-loop coverage, offline docs with isolated config/unreachable service, and unknown-topic behavior. These tests were read, not rerun; no new claim about Flowspace3's full suite is made.
- Index status reported a conversation-ingestion backlog, not a failure of the code hits used here. No absence claim depends on that backlog, and no service/config repair was attempted.

Reuse the design ideas, not Flowspace3's daemon/database architecture. Strengthen its top-level-command check for Unisphere: full nested command paths, options and executable synthetic examples matter here.

## Proposed docs commands

```text
unisphere docs
  list                 Discover the bundled operating guides
  get TOPIC            Read one complete version-matched guide
```

These are planned additions, not currently installed commands. Root `--help` points to `docs get start` and `docs get agents`. No separate custom query language, documentation server, browser or model service is required. Start with list/get; do not add a docs search index unless the topic set actually becomes hard to navigate.

- `docs list --json`: existing versioned command envelope with ordered `topics: [{name,title,summary,bytes}]` and a concrete next action.
- `docs get TOPIC --json`: envelope with `topic,title,text,related,cli_version`; Markdown is a complete string, not paginated fragments.
- `--human`: readable Markdown/text with no machine envelope. Piped machine mode remains predictable and explicitly overridable.
- Unknown topic: invalid-argument exit2, safe structured error including valid topic IDs and a `docs list` recovery action. No source scan or configuration repair.
- `docs` must work outside a checkout, with no agent stores, no Git/Git AI, malformed or missing Unisphere runtime configuration, no network, and no daemon. Route it before constructing optional storage/query dependencies.

## Topic map: purpose before syntax

| Topic | User question and outcome | Required material |
|---|---|---|
| start | What is this tool and how do I get one useful result? | Install/version, explicit repo discovery, one small inspection, limits and next pages |
| agents | How should an agent operate Unisphere without guessing or leaking content? | Scope, machine envelope, pagination, exit/partial semantics, privacy, action loop and recovery |
| find-sessions | Where did my repo's agent work go? | Usual stores versus checkout, exact/tree/worktree associations, source gaps, IDs versus source copies |
| inspect-conversations | What happened in this session? | Sessions, turns, messages, subagents/forks, incomplete boundaries and source provenance |
| filter-time-and-text | How do I select the intended evidence? | AND/OR, names/IDs/harness/model, event versus session time, half-open ranges, missing fields, literal versus regex |
| extract-context | How do I share only the useful part? | Match/context distinction, neighbour expansion, date-window expansion, content opt-in, create-new output, format choice |
| tool-analysis | Which tools failed or were slow? | Call/result pairing, shell family versus native names, missing durations, correct statistical denominators and sample pipelines |
| output-and-schema | How do scripts consume results without scraping tables? | JSON/JSONL/CSV contracts, schema show, null/units/projection, continuation, stdout versus stderr, offline input |
| privacy-and-coverage | What will be read, emitted and not known? | Payload boundaries, content canaries, source capabilities, unavailable versus empty, no full-fidelity/exactly-once claim |
| sdk | How do I embed the same behavior? | Small compile-and-run Rust examples using public APIs, explicit injected sources, output/error handling and no CLI subprocess dependency |
| troubleshooting | Why am I seeing nothing, duplicates, an error or stale results? | Wrong source root, unassociated metadata, unknown IDs, stale cursors, missing permissions/tools, unsupported format, partial output |
| git-ai-notes | How does commit attribution relate to my sessions? | The landed plan014 interface when available; Git AI not required, source facts only, no retired harness telemetry |

Every page starts with a real question, the decision/action it supports, prerequisites and a bounded working example. Then: expected result, interpretation, what not to infer, common failure/empty cases and linked next steps. Include the user's journey, not only a table of flags.

The Git-ai page is conditional on plan014's actual public contract; docs must not publish speculative commands. The CLI plan can proceed against an interface stub/fake for integration design, but shipping docs and integration must target the reviewed landed contract.

## One truth per kind of documentation

- Command/argument/type facts come from the executable's parser/schema registry. Do not maintain a second handwritten flag registry that can agree with itself while the CLI differs.
- Bundled Markdown owns concise operating instructions. README is the entry page and links to these concepts; longer repository docs own design rationale and contributor detail. Generate or link shared syntax rather than copy it into three independently edited locations.
- Examples use a maintained synthetic fixture corpus and machine-readable cases with argv, input fixture, expected behavior, relevant output fields, exit code and privacy policy. Test output semantics, not arbitrary prose formatting.
- Include files within the publishing crate/package boundary. Test an installed/package artifact outside the repository; successful developer-checkout reads are not packaging proof.
- `schema show` supplies fields, units, availability and sensitivity. Docs teach how to use that schema, not independently redeclare every field.
- Document source-format versions and observations. Do not treat an upstream harness privacy setting or an external project's parser as proof of Unisphere's behavior.

## Required deterministic proof

| Promise | Proof that can falsify it |
|---|---|
| Docs work before source access | Run the installed CLI in a fresh empty HOME/cwd with unavailable Git/Git AI and invalid runtime config; docs list/get still work and no loader/service is opened |
| A topic typo is recoverable | Unknown topic returns exit2, valid IDs and a next action; no empty success or panic |
| Registry and related links are coherent | Every related topic resolves; topic IDs are unique; list/get agree on actual registered topics |
| Examples use real grammar | Parse every full example command and option with the real CLI parser, not a duplicate list of accepted verbs |
| Workflows do what the docs say | Execute representative composed workflows on synthetic source fixtures and compare selected IDs/counts/timing/privacy/exit behavior |
| SDK docs are executable | Compile and run every public SDK example from an external temporary consumer against the release candidate |
| Bundling survives packaging | Execute docs from a temporary installed binary after removing access to the source checkout |
| Privacy claims survive modes | Synthetic sentinel content cannot appear in default JSON, JSONL, tables, CSV, Markdown errors or diagnostics; explicit content output is tested separately |
| Output examples are truthful | Expected JSON validates and numeric summaries are derived from the fixture, with missing data and units explicit |
| Changes cannot silently rot docs | CI's normal PR lane executes docs/example checks; removal/rename of a taught nested verb/flag or a schema change breaks the relevant proof |

Existing tests that only pin help wording or incidental formatting should not be multiplied. Keep guards that defend actual availability, privacy, grammar and consumer behavior. No passing documentation-source grep alone can prove an extraction workflow.

## Delivery checklist for the future PM

1. Name a docs owner in the guide, with the same status as discovery, query and tool-analysis owners.
2. Freeze topic/command/schema interfaces so the docs owner can use fakes/fixtures while handlers are implemented independently.
3. Require each command owner to supply its question, example, expected interpretation, failure example and source limitation with its delivery.
4. Integrate docs and examples in the normal build/PR proof; do not make them an optional final polish lane.
5. Demonstrate a fresh human/agent journey from `--help` to a correct, bounded extraction without reading repository internals.

Public reference: https://github.com/AI-Substrate/flowspace3. Implementation/test paths above are the evidence anchors; no internal ISE findings or private transcript data belongs in this plan.
