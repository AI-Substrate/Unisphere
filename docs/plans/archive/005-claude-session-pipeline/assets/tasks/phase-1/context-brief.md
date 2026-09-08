# Pipeline implementation context

Canonical product plan: ../../../plan.dd.json. Approved implementation guide: ../../impl-guide.dd.json. All exact types, semantics, scopes and check commands live there; this file is a routing brief, not a second contract.

PM baseline tk-0001 provides real core collection types/ports/errors, public validation, FakeSessionLoader, FakeRecordWriter, FakeCollector, TextFixtureAdapter, fixture_records and assert_adapter_conformance. All shared exports are available through unisphere_core and testkit::collection. Native loader, pure Claude adapter and OTLP writer are separate baseline-only lanes; none reads sibling implementation. PM integrates SDK Collector/collect_batch, CLI run_sessions, static app selection and full proof afterward.

No adapter filesystem/environment/clock/network access. Inputs are explicit bounded byte batches; content is metadata-only by default, opt-in otherwise. No native private store proof. Required invariants: partial LF cursor, oversized-record retry, no silent resets, checkpoint only after output accepted, bounded32MiB encoding, native usage snapshots rather than invented standard totals, exact OTLP profile with source provenance. File loader Unix-only; pure mapper/writer portable.

Coders author their complete code/tests/docs and scoped commits but skip formatter/build/lint/tests; PM runs those once at integration and returns concrete defects. Never deliver root Cargo.lock, shared contract changes or canonical records. Requested native OMP GitHub Astra/high, independent fresh Opus5/high reviewer. Follow current Builder native packet/ack/release evidence; preserve real scope and proof when reporting dogfood friction.
