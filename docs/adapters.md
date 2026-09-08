# Adding a pure session adapter

An adapter receives `SessionRef`, a slice of complete `NativeRecord` values and
`MappingOptions`, then returns `MappedBatch` or a typed `PipelineError`. It does
not get a loader, file handle, clock, environment or destination. Storage loading
and output encoding are separate ports composed by the SDK `Collector`.

## Smallest working example

`unisphere_testkit::collection::TextFixtureAdapter` is a complete functional
example: every provided UTF-8 native record becomes a text-fragment record with
source provenance; metadata-only omits the body and content opt-in includes it.
It is not a production fallback for unknown Claude input.

The shared conformance helper is callable directly:

```rust
use unisphere_core::SessionRef;
use unisphere_testkit::collection::{
    TextFixtureAdapter, assert_adapter_conformance, fixture_records,
};

let source = SessionRef { path: "/synthetic/session.jsonl".into() };
let records = fixture_records(b"SENSITIVE-EXAMPLE\nsecond record\n");
assert_adapter_conformance(&TextFixtureAdapter, &source, &records);
```

For a new ordinary adapter:

1. Implement `SessionAdapter` in an isolated package or module using only supplied
   data; use the functional text example as the starting shape, not a `todo!` stub.
2. Give it a stable `name()`, native fixtures and explicit expected mappings,
   including unsupported data, malformed input and metadata/content policy.
3. Run `assert_adapter_conformance` on supported complete fixture records and add
   the format-specific edge cases the common suite cannot know.
4. Register the implementation in the app's explicit adapter factory and declare
   its inward dependencies in Cargo and the architecture policy; no core or
   existing adapter implementation change is needed for another mapper fitting
   the current record contract.
5. Compose it with an existing loader and `OtlpJsonlWriter`, or an injected test
   loader/writer, using `Collector::new(loader, adapter, writer)`.

No dynamic library loading, reflection or hidden service locator is involved.
A genuinely different storage/resumption model may require a new loader contract;
the current file cursor is not a promise that SQLite/API state can be represented
by an invented file offset.

## What conformance proves

The adapter helper checks deterministic mapping, record-count bounds, stable
source provenance, metadata-only body omission and the reserved `SENSITIVE-*`
fixture markers. Put those markers in fixture content, not legitimate metadata.
The writer separately enforces `MAX_OUTPUT_BATCH_BYTES` on actual OTLP-encoded
bytes; serializing the Rust DTO would measure a different representation and is
not proof of that limit.

Blank JSONL framing means ASCII-whitespace-only physical lines, including spaces,
tabs, CR and LF. The loader consumes their physical byte/record budget and advances
the read position but does not pass them to the mapper. It defers an incomplete
final line without skipping it. Record-limit failures retain previous progress;
retry with explicitly larger compatible limits rather than silently dropping data.

## Purity checks and their ceiling

The architecture command checks normal/dev/build dependency declarations and scans
core and Claude-adapter production sources for filesystem, environment, process,
network, thread, clock, unsafe/FFI and file-include access. Negative fixtures cover
fully qualified and grouped imports and clock/include calls; positive fixtures
permit `#![forbid(unsafe_code)]` and core-owned `std::io::Write` port declarations.
Testkit is not a production mapper and is excluded; conventional trailing
`#[cfg(test)]` modules are excluded from the source scan.

This is a lexical sensor, not a formal Rust effect system: aliases, macro expansion,
indirect calls, unusual test layout and source strings can exceed its precision.
Independent source review complements it. It must not succeed after examining zero
core sources. The mapper's strongest practical test remains that fixture bytes can
be mapped directly with no runtime environment or mock filesystem.

The normative output namespace and pinned schema references live in
[telemetry-profile.md](telemetry-profile.md); native extraction rules live in
[claude-adapter.md](claude-adapter.md). Native record, logical message and completed
model invocation are distinct entities: do not silently deduplicate or invent
usage totals merely to fit a consumer's presentation.
