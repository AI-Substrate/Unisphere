# Privacy, provenance, and coverage

## Motivating question

What did Unisphere actually inspect and emit, and what must remain an explicit human interpretation?

## Prerequisites and example

```sh
unisphere sources list --repo . --format json
unisphere messages list --repo . --contains rollback --format json
unisphere messages extract --repo . --contains rollback \
  --include-content --format jsonl
```

The list search authorizes local comparison against message text, not emission. The extract separately opts in to approved content output.

## Evidence supplied by Unisphere

- local query/source/entity IDs and revision-qualified source references;
- read and repository-association facts;
- field availability and content-omission reasons;
- matched/emitted counts, saved-input universe and continuation state;
- source-qualified reconstruction, outcome and duration basis;
- declared deterministic calculations.

## Interpretation left to humans or downstream systems

- whether a source is the complete history of work;
- whether omitted data contained secrets;
- why a tool failed or whether a session was productive;
- whether equal text/IDs identify the same work;
- monetary cost, causality, quality or novelty.

Metadata is not anonymity. Names, native IDs/models/tool names, source/native-key paths, message/reasoning parts, commands, inputs/results, identities and free-form attributes are sensitive. `--include-content` is deliberate consent, not a claim that emitted content is safe to publish. Unisphere follows no URLs, attachments or sidecars implicitly.

Coverage describes source reading/mapping/admission. Universe describes this response/saved selection. Partial evidence stays labelled; `--allow-partial` accepts named read failures but does not make them complete. Snapshot projections are current replacements, not persistent history or source finality.

## Limits and recovery

`UNI-QUERY-CONTENT-CONSENT` requires metadata-only columns or deliberate consent. `UNI-QUERY-SOURCE-READ` requires changing access/stability or selecting another source; blind retry is appropriate only after the named prerequisite changes. `UNI-QUERY-VIEW-SCOPE` requires reopening a wider view explicitly. No recovery action echoes hostile payloads or weakens privacy automatically.

**Next step:** inspect `data.coverage`, `data.universe`, and the diagnostic next action together before using results.
