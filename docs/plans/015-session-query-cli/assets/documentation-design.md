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
- `docs get TOPIC --json`: envelope with `topic,title,text,related,cli_version` and `next_action`; Markdown is a complete string, not paginated fragments.
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

Every command's documentation must carry useful use cases, not merely a generic description of its operation. Include creative applications and command combinations where they solve a concrete user problem; distinguish what Unisphere selects or measures from interpretation or downstream work performed by the user or another tool. Creativity does not authorise speculative capabilities, unsupported causal claims or additional product scope. Public research can inform these examples, but externally observed practices must be distinguished from proposed applications and verified against primary sources.

Jordan confirmed recipe-based documentation: “recipes are sweet. Let's do that.” Recipes should compose commands into useful end-to-end outcomes while retaining each command's own use cases. He also suggested companion skills, possibly shipped through an “agent file”; whether skills ship, their exact scope and the packaging/install mechanism remain under discussion. A candidate skill may guide an agent through a documented recipe using the CLI, without duplicating SDK semantics or turning optional downstream interpretation into a mandatory product dependency.

The Git-ai page is conditional on plan014's actual public contract; docs must not publish speculative commands. The CLI plan can proceed against an interface stub/fake for integration design, but shipping docs and integration must target the reviewed landed contract.

## One truth per kind of documentation

- Command/argument/type facts come from the executable's parser/schema registry. Do not maintain a second handwritten flag registry that can agree with itself while the CLI differs.
- Bundled Markdown owns concise operating instructions. README is the entry page and links to these concepts; longer repository docs own design rationale and contributor detail. Generate or link shared syntax rather than copy it into three independently edited locations.
- Examples use a maintained synthetic fixture corpus and machine-readable cases with argv, input fixture, expected behavior, relevant output fields, exit code and privacy policy. Test output semantics, not arbitrary prose formatting.
- Include files within the publishing crate/package boundary. Test an installed/package artifact outside the repository; successful developer-checkout reads are not packaging proof.
- `schema show` supplies fields, units, availability and sensitivity. Docs teach how to use that schema, not independently redeclare every field.
- Document source-format versions and observations. Do not treat an upstream harness privacy setting or an external project's parser as proof of Unisphere's behavior.
- Every command outcome provides a useful next action, and every error provides safe cause-specific recovery; the shared contract is in `query-contract.md#next-actions-and-actionable-errors`. Recipes teach these transitions, including zero matches, partial coverage and failures, rather than leaving users at a dead end.

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
| Every outcome has a useful next step | Exercise every command leaf plus help/version, zero-match and partial outcomes; validate suggested command grammar with the actual parser and run representative follow-on steps without contaminating data streams |
| Every error is actionable | Trigger invalid selection, input, permissions, identity, continuation, privacy, bound and output failures; check cause-specific recovery, valid alternatives/retry conditions and privacy-safe diagnostics rather than exact prose |

Existing tests that only pin help wording or incidental formatting should not be multiplied. Keep guards that defend actual availability, privacy, grammar and consumer behavior. No passing documentation-source grep alone can prove an extraction workflow.

## Delivery checklist for the future PM

1. Name a docs owner in the guide, with the same status as discovery, query and tool-analysis owners.
2. Freeze topic/command/schema interfaces so the docs owner can use fakes/fixtures while handlers are implemented independently.
3. Require each command owner to supply its question, example, expected interpretation, failure example and source limitation with its delivery.
4. Integrate docs and examples in the normal build/PR proof; do not make them an optional final polish lane.
5. Demonstrate a fresh human/agent journey from `--help` to a correct, bounded extraction without reading repository internals.

Public reference: https://github.com/AI-Substrate/flowspace3. Implementation/test paths above are the evidence anchors; no internal ISE findings or private transcript data belongs in this plan.

## Research-inspired recipe candidates

Jordan requested creative, useful applications of agent/session telemetry beyond routine monitoring. Discovery used Perplexity MCP search; the primary pages below were read directly. External tools, notebooks and research experiments were not run. These are proposed applications of the planned command surface, not extra commands, implementation approval or claims of demonstrated Unisphere behavior.

| User question | Proposed Unisphere recipe | Evidence and boundary |
|---|---|---|
| What do I keep having to correct? | Find candidate user messages with `messages list`; inspect their episodes with `turns show`; use `turns extract` with context to prepare examples for a human-reviewed rule or skill. | R1 studies correction-grounded episodes; R2 distils skills from interaction histories. Literal matches are candidates, not an automatic misalignment classifier; one-off instructions are not durable preferences. |
| How did we solve that once-a-quarter problem? | Locate relevant sessions and message text, inspect the successful and failed tool calls, then extract the useful episode rather than replaying it. | R3 demonstrates retrieval of a prior token-refresh procedure after recording a checkpoint; the earlier uncommitted attempt was unavailable. Recover the procedure, not credentials, and revalidate old commands before any separate execution. |
| Why did we reject the obvious approach? | Use message search to locate the recorded discussion; inspect the selected session and any evidenced branches with `sessions show/tree`; extract the actual rejection and its context. | R4 reports a handover practice preserving rejected approaches. This mapping is our proposal: rejection and rationale require explicit recorded evidence, not inference from an abandoned branch. |
| Can this failure become a regression case? | Select failed calls with `tools list`, inspect their surrounding turns, and export a bounded episode plus provenance for a separate evaluation workflow. | R5 provides a runnable trace/feedback-to-evals cookbook for a financial agent, not a coding-agent collector. Ground-truth labels, environment fixtures, judges and execution remain external work; an extract is not yet a replayable test. |
| Did the agent actually do what it claimed? | Compare selected assistant messages with `tools show` and `events show`, retaining source revision and availability evidence in an extract. | R1 studies inaccurate self-reporting; R6 documents checking whether an agent read files or ran tests. Missing captured evidence is not proof that an action never happened, and a recorded test command is not proof of its result. |
| Where are we repeating the same unproductive loop? | Export tool calls and outcomes, inspect repeated attempts in their turns, and compare measured counts/durations with `tools stats`. | R6 documents retry/workflow inspection. Repeated command text does not establish shared invocation identity or waste; a human or downstream analysis must distinguish necessary retries, changed inputs and parallel calls. |
| Can the next session start with evidence instead of a transcript dump? | Select the relevant session/turn slice and extract a small context packet; let the author write the current state, decisions and next steps with references to that evidence. | R4 reports this layered handover practice and distinguishes a transcript from a briefing. Unisphere supplies the selected evidence, not an automatically authored summary or session-resume mechanism. |
| What is missing from the story before I trust an analysis? | Start with `adapters list`, `sources list/check` and `schema show`, then inspect unresolved source events before interpreting session or turn statistics. | R7 explicitly documents missing assembled context in hook traces. This is our proposed coverage recipe: a skill mention is not proof of its loaded contents, and absent or unsupported evidence must not become a zero or a completeness claim. |

### Primary references and evidence grades

- **R1 — observational research:** [How Coding Agents Fail Their Users](https://arxiv.org/html/2605.29442v1), sections 3.2–3.3, 4.1 and Limitations. Episodes are grounded in developer pushback; the authors explicitly discuss missing context, false positives, public-data selection bias and non-causal comparisons.
- **R2 — experimental research:** [Do Personalized Skills Help Coding Agents?](https://arxiv.org/html/2608.10319v2), sections 2.2 and 3.1–3.3. The filtered study uses 206 sessions from 13 developers and simulated follow-ups; personalised gains are inconsistent and not statistically significant. Do not advertise automatic improvement from extracting preferences.
- **R3 — vendor-affiliated practitioner demonstration:** [How to Make Coding Agents Remember Past Solutions](https://dev.to/entire/how-to-make-coding-agents-remember-past-solutions-4a71), “Using my session history as an artifact” and “Here's how my agent responded.” A concrete reported retrieval walkthrough, not independent proof of universal retention or Unisphere's Git-ai integration.
- **R4 — practitioner report:** [The handover problem: agent sessions end, projects don't](https://jazzyalex.github.io/agent-sessions/blog/the-handover-problem/), “The pattern” and “Why not just resume the session?” Reports a dated state/decisions/next briefing layered over searchable transcripts; its absolute claims about Git/transcript contents are not adopted here.
- **R5 — official executable example, inspected not run:** [Build an Agent Improvement Loop with Traces, Evals, and Codex](https://developers.openai.com/cookbook/examples/agents_sdk/agent_improvement_loop). General-agent analogy requiring live models, feedback and an external evaluation stack, not evidence those capabilities belong in this CLI.
- **R6 — vendor documentation and proposed workflows:** [Coding agent tracing and evaluation](https://arize.com/blog/open-source-coding-agent-tracing/), “What you can inspect in a trace” and “Improving your coding agent workflow.” The article explicitly calls its comparisons examples in a new domain; it does not establish productivity gains or causal model rankings.
- **R7 — vendor integration documentation:** [Tracing coding agents with Langfuse](https://langfuse.com/resources/engineering/coding-agent-tracing), “What the traces let you do,” “Limits” and the skills FAQ. Useful evidence of documented workflows and stated blind spots, not source-format authority or proof of complete capture.

For delivery, turn selected recipes into synthetic, executable command examples under the existing docs proof contract. Retain every command's own practical use cases, including learning/configuration/schema commands; not every leaf needs a dramatic narrative. Keep provenance, content consent and publication review explicit. Public availability of a transcript does not itself authorise republishing it or incorporating it into a dataset; no external transcript or code was copied into these examples.
