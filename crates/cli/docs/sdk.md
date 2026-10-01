# Rust SDK query recipes

## Motivating question

How can an external Rust consumer use the same typed schemas and query semantics without invoking the CLI or allowing ambient filesystem access?

## Prerequisites

Depend on `unisphere-sdk`. Query contracts are re-exported under `unisphere_sdk::query`. Provide `QuerySource` explicitly to `QueryService`; construction performs no discovery. Relative local paths are resolved by the caller/composition boundary before a `QueryRequest` reaches the SDK.

Packaged examples:

- `crates/testkit/fixtures/query-docs/sdk-schema.rs` discovers fields/formats with no provider.
- `crates/testkit/fixtures/query-docs/sdk-query.rs` builds a validated request and invokes an injected `QueryApi`.

```rust
use unisphere_sdk::query::{schema, Dataset, FieldId};

let tools = schema(Dataset::Tools);
assert!(tools.field(FieldId::DurationMs).is_some());
assert!(tools.field(FieldId::P95Ms).is_some());
let turns = schema(Dataset::Turns);
assert!(turns.field(FieldId::IsContext).is_some());
```

Expected: the schema is static core data; it performs no source/config/Git/network access. Dataset schemas list projected fields as well as predicate/group/order support: stats result fields such as `p95_ms` are discoverable even though they are not input columns, and turn/message `is_context` is metadata-only output rather than a predicate. `QueryApi::execute` returns approved projected rows plus coverage, universe and a typed semantic `QueryAction`. The CLI alone renders actions into argv through its parser grammar; SDK consumers should not fabricate shell strings.

## Limits and interpretation

`QueryView` is an explicit in-process evidence API, not serializable output. Repeated pure queries can narrow an admitted view but cannot widen scope or request unretained fields. `QueryResponse` is projected output, not a raw native graph. A consumer must preserve content consent and source-qualified availability.

## Recovery

Match `QueryFailureCode` and `RecoveryAction`. Use returned allowed fields/adapters/entities/branches rather than parsing message text. Supply complete versioned input for unavailable offline context; reopen a view for wider live scope; change an output destination after output failure.

**Next step:** compile and run the packaged schema example, then substitute your own injected `QuerySource`.
