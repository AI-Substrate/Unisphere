# Design inputs and deliberate limits

- User boundary: pure adapters have no filesystem access; shared injectable SessionLoader owns list_sessions/read_batch, mapping returns contracted records; output separated.
- Prior selected direction: standard-first OTLP LogsData JSONL with GenAI fields where their meanings actually apply, not a second canonical persisted encoding.
- Archived workshop source: ../archive/001-sdk-cli-foundation/assets/workshops/001-output-format.md (from docs/plans); mapping tables distinguish physical record, logical message, inference invocation, provider, source and pij identity.
- Scratch Claude source and tests preserve every physical record; sanitized fixture has six assistant records sharing only two message IDs, each repeated three times; sidecar sessionId may be parent identity; persistedOutputPath is never opened.
- Shared scratch framer bounds physical lines and resumes after complete LF, defers partialUTF8/tail, identifies Unix rotation/truncation, and explicitly cannot detect arbitrary in-place rewrite/regrowth. New product loader rejects incompatible cursors rather than silently resets them.
- Primary encoding source read: https://raw.githubusercontent.com/open-telemetry/opentelemetry-proto/main/docs/specification.md (JSON Protobuf Encoding): lowerCamelCase, integer enums, int64/u64 decimal strings; trace/span hex rules if ever used, but this slice invents none.
- Proposed profile event unisphere.session.record is explicitly nonstandard and source-derived; standard-shaped message body with role/parts does not claim full GenAI inference histories or output choices.
- Native usage stays explicitly named snapshot-component metadata until exact Claude/provider cached-total semantics are proved; repeated IDs must not be summed or deduplicated by guesswork.
- Conservative initial policy: no body/content export unless include_content is explicit; source/model/id metadata can itself be sensitive and is documented as such, not claimed anonymous.
- First loader listing is nonrecursive within explicit project directory; caller may select an explicit sidecar file, but no implicit cross-project or attachment discovery.
- Existing configuration behavior, directSDK usability and installedCLI no-Node runtime remain regression requirements.

These inputs justify the proposed contracts; independent review and actual implementation proof are still required. No private user store was read.
