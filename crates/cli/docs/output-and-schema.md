# Output formats and schema discovery

## Motivating question

Which output format preserves the distinctions my consumer needs, and where does guidance appear without contaminating data?

## Prerequisites and example

```sh
unisphere schema show messages --json
unisphere messages list --input synthetic-query.json \
  --columns id,session_id,role,timestamp --format jsonl
```

Expected: schema output declares types, nullability, units, sensitivity, availability, predicates, operations, defaults, grouping/metrics, formats and losses. Query JSONL contains only typed row records on stdout; coverage and next action use stderr or an explicit manifest.

## Format interpretation

- JSON: one versioned command envelope. `data.coverage` and `data.universe` are siblings. `next_action` is top-level.
- JSONL: one typed row per line. No banner, summary, action or fake trailer row.
- CSV: UTF-8 RFC4180 convenience projection. Null/absence/empty may collapse; compound values are JSON text. Spreadsheet safety is default; `raw` preserves strings but not content consent.
- Table: human presentation; rounding and layout are not parser contracts.
- Text/Markdown: extraction presentations with separate session/branch groups; content-sensitive and lossy.
- OTLP: only native `sessions export`; query formats never masquerade as LogsData.

Timestamps preserve normalized instants but serialized formats do not preserve in-memory timestamp evidence basis. Inspect schema losses before choosing a format.

## Limits and recovery

A field unknown to the selected dataset returns `UNI-QUERY-FIELD` plus valid alternatives and `unisphere schema show DATASET`. An unsupported format/operation returns `UNI-QUERY-OPERATION`. CSV diagnostics recommend JSON/JSONL when exact absence/null distinctions matter. `--json`, `--human`, and query `--format` conflict rather than silently taking precedence.

A file output is create-new. On `UNI-QUERY-OUTPUT`, choose a new writable destination and discard partial output; retry only after the destination changes. A closed stdout pipe remains nonzero and no completion manifest is emitted.

**Next step:** choose the least lossy schema-declared format that your consumer can process.
