# Filter by time, metadata, and text

## Motivating question

Which evidence matches a precise time, identity, metadata, or payload condition without treating missing values as matches?

## Prerequisites

Run `unisphere schema show DATASET --json`. Different fields combine with AND; repeated values of one field combine with OR; exclusions subtract last. Filter support and field availability are dataset- and source-qualified.

## Recipe 3 — locate a request in a half-open time window

```sh
unisphere messages list --repo . --role user \
  --since 2026-09-01 --until 2026-09-02 \
  --contains rollback --format json
```

Expected: `since` is inclusive and `until` exclusive at UTC midnight for date-only values. The literal is inspected locally, but `text` is not emitted without `--include-content`. Unknown timestamps do not match a positive time comparison unless `--include-undated` is explicit.

Interpretation: the command supplies matching local IDs, role/timestamp evidence and coverage. It does not prove the request caused later work. A session-time query and an event-time query answer different questions.

## Filter choices

- `--contains TEXT`: literal search.
- `--regex PATTERN`: explicit bounded regular expression; no code/shell/SQL evaluation.
- `--name GLOB`: glob over recorded sensitive names; comparison consent is not output consent.
- `--ignore-case`: changes comparison explicitly.
- `--sort FIELD` / `--sort=-FIELD`: stable order with missing values last and local ID tie-break.
- `--columns FIELDS`: declared projection only; reserved identity/provenance fields remain.
- `--limit 0`: all matches within declared safety bounds, not unlimited resources.

## Limits and recovery

Locale dates and natural-language relative dates fail with `UNI-QUERY-TIME`; use RFC3339 or `YYYY-MM-DD`. Invalid/oversized patterns fail with `UNI-QUERY-PATTERN`; correct or narrow them before retrying. An unsupported field fails with `UNI-QUERY-FIELD` and points to `schema show`; zero matches is not a field error.

**Next step:** bind a returned row ID for `show`, or use `extract` with deliberate content consent.
