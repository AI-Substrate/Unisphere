# Analyse tool outcomes and durations

## Motivating question

Which observed invocations failed or remained incomplete, and what timing statistics are justified by measured evidence?

## Prerequisites

Use `tools list` to inspect statuses and coverage before aggregation. Pairing requires supported native identity inside the correct session/participant/branch; adjacency or equal command text is never enough.

## Recipe 5 — investigate a failure hotspot

```sh
unisphere tools stats --repo . --group-by tool_family \
  --metric count --metric measured_count --metric missing_duration_count \
  --metric failures --metric incomplete --metric unknown \
  --metric failure_rate --metric p95_ms --format json
unisphere tools list --repo . --tool-family shell --status failed --format json
unisphere tools show "$TOOL_ID" --repo . --include-content --format json
```

Expected: one invocation is one row. Results distinguish `succeeded`, `failed`, `cancelled`, `incomplete`, and `unknown`. Duration is present only when source-reported or derived from a valid matched clock pair; the basis remains visible. Percentiles use exact nearest-rank over finite measured values.

Interpretation: Unisphere supplies observed outcomes, qualified durations, denominators and missing-sample counts. A human investigates root cause. A grouped failure rate is not agent quality, productivity, causal comparison, or proof that an unmeasured call was fast.

## Arithmetic limits

Failure-rate denominator is known terminal outcomes; incomplete/unknown calls are excluded but counted. Missing duration is null, never zero, and excluded from mean/percentiles while counted. Overlapping call durations cannot be read as wall-clock session time. Cumulative/replayed usage is not blindly summed. No monetary cost estimate is produced.

## Recovery

Missing metric fields in offline input require a new versioned extraction retaining those fields. An unsupported metric/group returns schema guidance. Content consent is required for command/input/output; use metadata-only `tool_family`, status, timing and local IDs when payload review is unnecessary.

**Next step:** inspect matching calls and their parent turns before drawing a conclusion.
