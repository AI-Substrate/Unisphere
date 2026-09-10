# Using Unisphere from an agent

## Motivating question

How can an agent inspect evidence predictably without scraping prose, leaking payloads, or allowing a suggestion to run automatically?

## Prerequisites

Use the installed binary and one explicit scope. Discover grammar through `--help`, topics through `docs list`, and fields through `schema show`. Treat any named input in `required_inputs` as a binding to obtain from an earlier result, not as an executable placeholder.

## Recipe 6 — analyse a saved view offline

```sh
unisphere sessions list --input synthetic-query.json --format json
unisphere tools stats --input synthetic-query.json --group-by tool_family \
  --metric count --metric failures --metric missing_duration_count --format json
```

Expected: both commands use only the supplied versioned document. `data.universe.bounded_by_input` states the boundary. No live source, Git, configuration, HOME, or network enrichment occurs.

Interpretation: an exact statistic over provided rows is not necessarily an exact statistic over the original source. Standalone JSONL has unknown original completeness unless validated completeness metadata accompanies it.

## Machine contract

- JSON responses are one envelope with `ok`, `command`, `v`, `data` or `error`, and nonempty `next_action`.
- JSONL, CSV, text, Markdown and OTLP stdout are clean data. Coverage, warnings and the next action use stderr or a separately requested manifest.
- Exit 0 includes zero matches. Exit 1 is operational/read/output failure. Exit 2 is invalid arguments.
- Do not infer success from output bytes alone; a late output error makes stdout incomplete.
- Parse `next_action.argv` only through the same installed CLI grammar. Bind `required_inputs` explicitly and present the command to the user; never execute it automatically.

## Limits and recovery

A stale cursor requires the original options and unchanged view, or a fresh query without the cursor. Missing offline fields require a complete versioned JSON extraction; live rescanning is never implicit. Content-denied fields require either metadata-only columns or deliberate `--include-content`, never automatic relaxation.

**Next step:** inspect `unisphere schema show DATASET --json` before generating a query.
