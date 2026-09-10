# Extract review context

## Motivating question

How can I preserve a matching request plus nearby supported evidence without leaking unrelated branches or pretending the extract is complete history?

## Prerequisites

Use turns or messages. Content-bearing text/Markdown, message bodies, names, command strings, inputs and results require `--include-content`. Context requires native order and membership; offline input must retain complete required partitions.

## Recipe 4 — create a branch-qualified review packet

```sh
unisphere turns extract --repo . --has-errors \
  --context-before 1 --context-after 1 \
  --include-content --format markdown --output review.md
```

Expected: filtering happens first. Neighbours then expand independently inside each admitted session/branch. Overlapping windows merge. Turn/message JSON and JSONL rows carry metadata-only `is_context: false` for matches and `is_context: true` for added neighbours; human text/Markdown labels them `match` or `context`. `matched` and `emitted` can differ. Context may fall outside date filters deliberately. `is_context` is an output label, not a filter predicate or new CLI option.

Interpretation: Unisphere supplies the selected evidence, membership, labels and coverage. It does not claim a linear timeline across branches, a complete transcript, or the cause of an error. The reviewer decides relevance.

## Creative use cases

- Build a handoff packet around failed tool calls without exporting the whole conversation.
- Quote user/assistant exchanges around a search hit while retaining source refs.
- Use `--range 12:20` after selecting exactly one session and branch to review stable turn ordinals.
- Produce JSON/JSONL for downstream analysis and Markdown only for human reading.

## Limits and recovery

`--range` is one-based and inclusive and needs exactly one unambiguous session/branch. Context never crosses admitted repository/source/participant authority. An incomplete saved input returns `UNI-QUERY-INPUT-SUBSET`; supply a complete versioned JSON extraction or remove context. An existing `--output` destination fails; choose a new writable path. After a failure, discard partial bytes and do not treat them as complete.

**Next step:** inspect the coverage/action diagnostic channel before sharing the packet.
