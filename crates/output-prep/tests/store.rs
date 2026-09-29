//! ParquetPrepStore contract: publication order and failure recovery, the
//! single-writer lock, schema metadata, views.sql, and compaction leaving every
//! canonical view unchanged. Published Parquet is read back through parquet's
//! Arrow reader and arrow-json; the canonical-view rules of views.sql are
//! applied here in Rust (no SQL engine needed).

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Path, PathBuf},
};

use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use unisphere_core::{
    PipelineErrorKind, SourceIdentity,
    prep::{
        CacheWriteBasis, CallSighting, PREP_CHECKPOINT_FORMAT, PREP_TABLE_SCHEMA_VERSION,
        PrepCallRow, PrepCheckpoint, PrepEventKind, PrepEventRow, PrepReplaceReason, PrepRows,
        PrepSetState, PrepSourceKind, PrepSourceMeta, PrepSourceState, PrepSourceStatus, PrepState,
        PrepStore, PrepToolUseRow, PrepTriggerRow, PrepTurnRow, SessionFacts, ToolOutcome,
        ToolSighting, TurnOrigin,
    },
};
use unisphere_output_prep::{
    FACT_TABLES, META_SCHEMA_VERSION, META_TABLE, ParquetPrepStore, VIEWS,
};
use unisphere_testkit::prep::MemoryPrepStore;

const A: &str = "claude-code/default/p/a.jsonl";
const B: &str = "claude-code/default/p/b.jsonl";
const C: &str = "claude-code/default/q/subagents/agent-c.jsonl";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn source(key: &str, generation: u32, status: PrepSourceStatus) -> PrepSourceState {
    let file = key.trim_start_matches("claude-code/default/").to_owned();
    PrepSourceState {
        set: "claude-code/default".into(),
        path: PathBuf::from("/native").join(&file),
        file,
        kind: PrepSourceKind::Append,
        identity: SourceIdentity::Unix {
            device: 1,
            inode: u64::from(generation) + 10,
        },
        size: 1_000,
        mtime_ns: 7,
        offset: 900,
        anchor: Some("anchor".into()),
        revision: None,
        generation,
        meta: PrepSourceMeta {
            is_sub: key == C,
            agent_id: (key == C).then(|| "agent-c".into()),
            project: Some(if key == C { "q" } else { "p" }.into()),
        },
        checkpoint: PrepCheckpoint {
            format: PREP_CHECKPOINT_FORMAT,
            policy: "test-fold/1".into(),
            fold: Value::Null,
        },
        facts: SessionFacts {
            session_id: Some(format!("session-{generation}")),
            records: 3,
            ..SessionFacts::default()
        },
        status,
    }
}

/// The state an engine would hand to `commit` for run `runs`: the store's
/// committed parts plus the given sources.
fn next_state(store: &impl PrepStore, runs: u64, sources: &[(&str, u32)]) -> PrepState {
    let parts = store
        .state()
        .expect("state")
        .map(|s| s.parts)
        .unwrap_or_default();
    let status = |generation| {
        if generation > 1 {
            PrepSourceStatus::Replaced {
                reason: PrepReplaceReason::Rotated,
            }
        } else {
            PrepSourceStatus::Appended
        }
    };
    PrepState {
        table_schema_version: PREP_TABLE_SCHEMA_VERSION,
        checkpoint_format: PREP_CHECKPOINT_FORMAT,
        runs,
        parts,
        sets: BTreeMap::from([(
            "claude-code/default".to_owned(),
            PrepSetState {
                harness: "claude-code".into(),
                label: "default".into(),
                root: PathBuf::from("/native"),
                policy: "test-fold/1".into(),
            },
        )]),
        sources: sources
            .iter()
            .map(|(key, generation)| {
                (
                    (*key).to_owned(),
                    source(key, *generation, status(*generation)),
                )
            })
            .collect(),
    }
}

struct Call {
    source: &'static str,
    generation: u32,
    offset: Option<u64>,
    key: Option<&'static str>,
    ids: Option<(&'static str, &'static str)>,
    update: bool,
    ts_ms: i64,
    input: i64,
    output: i64,
    stop: Option<&'static str>,
    sidechain: bool,
}

impl Call {
    fn new(
        source: &'static str,
        generation: u32,
        offset: u64,
        ids: (&'static str, &'static str),
    ) -> Self {
        Self {
            source,
            generation,
            offset: Some(offset),
            key: None,
            ids: Some(ids),
            update: false,
            ts_ms: i64::try_from(offset).unwrap() * 10,
            input: 10,
            output: 1,
            stop: None,
            sidechain: false,
        }
    }
    fn row(self) -> PrepCallRow {
        PrepCallRow {
            source: self.source.into(),
            generation: self.generation,
            native_offset: self.offset,
            native_key: self.key.map(Into::into),
            sighting: if self.update {
                CallSighting::Update
            } else {
                CallSighting::First
            },
            msg_id: self.ids.map(|(m, _)| m.into()),
            request_id: self.ids.map(|(_, r)| r.into()),
            ts: Some(format!("t{}", self.ts_ms)),
            ts_ms: Some(self.ts_ms),
            model: Some("model-x".into()),
            stop_reason: self.stop.map(Into::into),
            input: Some(self.input),
            cw_1h: Some(3),
            cw_5m: Some(0),
            cache_read: Some(50),
            output: Some(self.output),
            cache_write_basis: CacheWriteBasis::Split,
            is_sidechain: self.sidechain,
            gap_ms: Some(-1),
            turn_no: Some(1),
            call_in_turn: Some(1),
            records: 1,
        }
    }
}

fn update(mut call: Call, input: i64, output: i64, stop: Option<&'static str>) -> PrepCallRow {
    call.update = true;
    call.input = input;
    call.output = output;
    call.stop = stop;
    call.row()
}

fn turn(source: &str, generation: u32, offset: u64, turn_no: i64) -> PrepTurnRow {
    PrepTurnRow {
        source: source.into(),
        generation,
        native_offset: Some(offset),
        native_key: None,
        turn_no,
        started_ts: None,
        started_ts_ms: Some(1),
        first_call_offset: Some(offset + 1),
        origin: TurnOrigin::Human,
        sender: None,
        pij_msg_id: None,
        opener_offset: Some(offset),
        opener_ts_ms: None,
        opener_chars: Some(12),
        body_key: None,
    }
}

fn trigger(source: &str, generation: u32, offset: u64) -> PrepTriggerRow {
    PrepTriggerRow {
        source: source.into(),
        generation,
        native_offset: Some(offset),
        native_key: None,
        ts: None,
        ts_ms: Some(5),
        kind: TurnOrigin::Peer,
        sender: Some("peer-1".into()),
        pij_msg_id: Some("m-1".into()),
        chars: 40,
        body_key: None,
        next_turn_no: 2,
        content_head: None,
    }
}

fn compaction(
    source: &str,
    generation: u32,
    offset: Option<u64>,
    key: Option<&str>,
    ts_ms: i64,
) -> PrepEventRow {
    PrepEventRow {
        source: source.into(),
        generation,
        native_offset: offset,
        native_key: key.map(Into::into),
        ts: None,
        ts_ms: Some(ts_ms),
        kind: PrepEventKind::Compaction,
        subkind: None,
        trigger: Some("auto".into()),
        model: None,
        pre_tokens: Some(150_000),
        post_tokens: None,
        duration_ms: Some(900),
        last_context: None,
        gap_ms: None,
        resets_at: None,
        resets_at_ms: None,
        turn_no: 1,
        body_key: None,
    }
}

fn tool(
    source: &str,
    generation: u32,
    offset: Option<u64>,
    id: Option<&str>,
    sighting: ToolSighting,
) -> PrepToolUseRow {
    let is_use = sighting == ToolSighting::Use;
    PrepToolUseRow {
        source: source.into(),
        generation,
        native_offset: offset,
        native_key: offset
            .is_none()
            .then(|| format!("key-{}", id.unwrap_or("none"))),
        sighting,
        tool_use_id: id.map(Into::into),
        call_msg_id: is_use.then(|| "m1".into()),
        ts: None,
        ts_ms: offset.map(|o| i64::try_from(o).unwrap()),
        name: is_use.then(|| "Bash".into()),
        family: is_use.then(|| "shell".into()),
        input_hash: is_use.then(|| "h".into()),
        input_bytes: is_use.then_some(20),
        result_offset: (!is_use).then_some(offset).flatten(),
        result_bytes: (!is_use).then_some(42),
        outcome: (!is_use).then_some(ToolOutcome::Ok),
        duration_ms: None,
        turn_no: Some(1),
    }
}

/// Four runs exercising appends, a replaced source, snapshot-style addresses,
/// id-less calls and tool uses, repeated call and tool-use sightings.
fn runs() -> Vec<(PrepRows, Vec<(&'static str, u32)>)> {
    let snapshot_call = |generation, key, ids, ts_ms| {
        Call {
            offset: None,
            key: Some(key),
            ts_ms,
            ..Call::new(B, generation, 0, ids)
        }
        .row()
    };
    let run1 = PrepRows {
        calls: vec![
            Call::new(A, 1, 0, ("m1", "r1")).row(),
            update(Call::new(A, 1, 100, ("m1", "r1")), 12, 7, Some("tool_use")),
            Call {
                ids: None,
                ..Call::new(A, 1, 300, ("", ""))
            }
            .row(),
            Call {
                ids: None,
                input: 11,
                ..Call::new(A, 1, 400, ("", ""))
            }
            .row(),
            snapshot_call(1, "k1", ("m9", "r9"), 400),
            snapshot_call(1, "k4", ("m10", "r10"), 600),
        ],
        turns: vec![turn(A, 1, 0, 1), turn(B, 1, 0, 1)],
        triggers: vec![trigger(A, 1, 0)],
        events: vec![
            compaction(A, 1, Some(150), None, 2_000),
            compaction(B, 1, None, Some("k3"), 500),
        ],
        tool_uses: vec![
            tool(A, 1, Some(110), Some("t1"), ToolSighting::Use),
            tool(A, 1, Some(250), Some("t1"), ToolSighting::Result),
            tool(A, 1, Some(260), None, ToolSighting::Use),
            tool(B, 1, None, Some("t9"), ToolSighting::Use),
        ],
    };
    let run2 = PrepRows {
        calls: vec![
            Call {
                input: 20,
                sidechain: true,
                ..Call::new(A, 1, 200, ("m2", "r2"))
            }
            .row(),
            Call::new(A, 1, 450, ("m3", "r3")).row(),
            update(Call::new(A, 1, 500, ("m2", "r2")), 25, 9, Some("end_turn")),
            snapshot_call(2, "k1", ("m9", "r9"), 400),
        ],
        turns: vec![turn(A, 1, 440, 2), turn(B, 2, 0, 1)],
        triggers: vec![trigger(B, 2, 0)],
        events: vec![compaction(B, 2, None, Some("k3"), 500)],
        tool_uses: vec![
            {
                let mut dup = tool(A, 1, Some(510), Some("t1"), ToolSighting::Result);
                dup.duration_ms = Some(7);
                dup.result_bytes = Some(99);
                dup
            },
            tool(B, 2, None, Some("t9"), ToolSighting::Result),
        ],
    };
    let run3 = PrepRows {
        calls: vec![update(Call::new(A, 1, 600, ("m1", "r1")), 11, 8, None)],
        ..PrepRows::default()
    };
    let run4 = PrepRows {
        calls: vec![
            update(
                Call::new(A, 1, 700, ("m1", "r1")),
                9,
                30,
                Some("max_tokens"),
            ),
            Call::new(C, 1, 0, ("m1", "r1")).row(),
        ],
        tool_uses: vec![tool(A, 1, Some(260), None, ToolSighting::Result)],
        ..PrepRows::default()
    };
    vec![
        (run1, vec![(A, 1), (B, 1)]),
        (run2, vec![(A, 1), (B, 2)]),
        (run3, vec![(A, 1), (B, 2)]),
        (run4, vec![(A, 1), (B, 2), (C, 1)]),
    ]
}

fn commit_run(store: &impl PrepStore, runs: u64, rows: &PrepRows, sources: &[(&str, u32)]) {
    let state = next_state(store, runs, sources);
    store.commit(rows, &state).expect("commit");
}

// ---------------------------------------------------------------------------
// Read-back and canonical views in Rust
// ---------------------------------------------------------------------------

fn read_rows<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(path).expect("open"))
        .expect("parquet")
        .build()
        .expect("reader");
    let mut rows = Vec::new();
    for batch in reader {
        let mut json = arrow_json::LineDelimitedWriter::new(Vec::new());
        json.write(&batch.expect("batch")).expect("json");
        json.finish().expect("json");
        rows.extend(
            serde_json::Deserializer::from_slice(&json.into_inner())
                .into_iter::<T>()
                .map(|r| r.expect("row")),
        );
    }
    rows
}

fn kv(path: &Path, key: &str) -> Option<String> {
    let builder =
        ParquetRecordBatchReaderBuilder::try_new(File::open(path).expect("open")).expect("parquet");
    builder
        .metadata()
        .file_metadata()
        .key_value_metadata()?
        .iter()
        .find(|kv| kv.key == key)?
        .value
        .clone()
}

fn published_state(target: &Path) -> PrepState {
    serde_json::from_slice(&fs::read(target.join("state.json")).expect("state.json")).expect("json")
}

/// `(coalesce(native_offset, -1), part file, row number)`: the views.sql native order.
type Order = (i64, String, usize);
/// Tool-use field priority: use sightings first, then native order.
type ToolOrder = (bool, Order);

/// Rows of `table` in the published parts, each with its native order key
/// `(coalesce(native_offset, -1), part file, row number)`.
fn positioned<T: DeserializeOwned>(
    target: &Path,
    parts: &[String],
    table: &str,
    offset: impl Fn(&T) -> Option<u64>,
) -> Vec<(T, Order)> {
    let mut files: Vec<&String> = parts
        .iter()
        .filter(|p| p.starts_with(&format!("tables/{table}/")))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for file in files {
        for (row, value) in read_rows::<T>(&target.join(file)).into_iter().enumerate() {
            let order = (offset(&value).map_or(-1, |o| o as i64), file.clone(), row);
            out.push((value, order));
        }
    }
    out
}

fn json_without(value: &impl Serialize, drop: &[&str]) -> Value {
    let mut value = serde_json::to_value(value).expect("json");
    for key in drop {
        value.as_object_mut().expect("object").remove(*key);
    }
    value
}

fn max_opt(values: impl Iterator<Item = Option<i64>>) -> Option<i64> {
    values.flatten().max()
}

/// Every canonical view, each as a sorted list of JSON rows.
fn views(target: &Path) -> BTreeMap<&'static str, Vec<String>> {
    let state = published_state(target);
    let parts = &state.parts;
    let sources: Vec<Value> = read_rows(&target.join("tables/sources.parquet"));
    let sessions: Vec<Value> = read_rows(&target.join("tables/sessions.parquet"));
    let current: BTreeSet<(String, u32)> = sources
        .iter()
        .map(|s| {
            (
                s["source"].as_str().unwrap().to_owned(),
                s["generation"].as_u64().unwrap() as u32,
            )
        })
        .collect();
    let is_current =
        |source: &str, generation: u32| current.contains(&(source.to_owned(), generation));

    // calls_v
    let mut groups: BTreeMap<String, Vec<(PrepCallRow, Order)>> = BTreeMap::new();
    for (row, order) in positioned::<PrepCallRow>(target, parts, "calls", |r| r.native_offset) {
        if !is_current(&row.source, row.generation) {
            continue;
        }
        let key = if row.msg_id.is_none() && row.request_id.is_none() {
            format!("solo {} {}", order.1, order.2)
        } else {
            serde_json::to_string(&(&row.source, row.generation, &row.msg_id, &row.request_id))
                .unwrap()
        };
        groups.entry(key).or_default().push((row, order));
    }
    let calls: Vec<PrepCallRow> = groups
        .into_values()
        .map(|mut group| {
            group.sort_by(|a, b| a.1.cmp(&b.1));
            let mut merged = group[0].0.clone();
            merged.input = max_opt(group.iter().map(|(r, _)| r.input));
            merged.cw_1h = max_opt(group.iter().map(|(r, _)| r.cw_1h));
            merged.cw_5m = max_opt(group.iter().map(|(r, _)| r.cw_5m));
            merged.cache_read = max_opt(group.iter().map(|(r, _)| r.cache_read));
            merged.output = max_opt(group.iter().map(|(r, _)| r.output));
            merged.stop_reason = group.iter().rev().find_map(|(r, _)| r.stop_reason.clone());
            merged.records = group.iter().map(|(r, _)| r.records).sum();
            merged
        })
        .collect();

    // tool_uses_v
    let mut tool_groups: BTreeMap<String, Vec<(Value, ToolOrder)>> = BTreeMap::new();
    for (row, order) in
        positioned::<PrepToolUseRow>(target, parts, "tool_uses", |r| r.native_offset)
    {
        if !is_current(&row.source, row.generation) {
            continue;
        }
        let key = match &row.tool_use_id {
            Some(id) => serde_json::to_string(&(&row.source, row.generation, id)).unwrap(),
            None => format!("solo {} {}", order.1, order.2),
        };
        let priority = (row.sighting != ToolSighting::Use, order.clone());
        tool_groups
            .entry(key)
            .or_default()
            .push((json_without(&row, &["sighting"]), priority));
    }
    let tool_uses: Vec<Value> = tool_groups
        .into_values()
        .map(|mut group| {
            group.sort_by(|a, b| a.1.cmp(&b.1));
            let mut merged = serde_json::Map::new();
            for (row, _) in &group {
                for (field, value) in row.as_object().unwrap() {
                    if !value.is_null() && !merged.contains_key(field) {
                        merged.insert(field.clone(), value.clone());
                    }
                }
            }
            Value::Object(merged)
        })
        .collect();

    fn current_of<T: DeserializeOwned>(
        target: &Path,
        parts: &[String],
        table: &str,
        key: impl Fn(&T) -> (String, u32),
        is_current: &dyn Fn(&str, u32) -> bool,
    ) -> Vec<T> {
        positioned::<T>(target, parts, table, |_| None)
            .into_iter()
            .map(|(row, _)| row)
            .filter(|row| {
                let (source, generation) = key(row);
                is_current(&source, generation)
            })
            .collect()
    }
    let turns: Vec<PrepTurnRow> = current_of(
        target,
        parts,
        "turns",
        |r: &PrepTurnRow| (r.source.clone(), r.generation),
        &is_current,
    );
    let triggers: Vec<PrepTriggerRow> = current_of(
        target,
        parts,
        "triggers",
        |r: &PrepTriggerRow| (r.source.clone(), r.generation),
        &is_current,
    );
    let events: Vec<PrepEventRow> = current_of(
        target,
        parts,
        "events",
        |r: &PrepEventRow| (r.source.clone(), r.generation),
        &is_current,
    );

    // compactions_v
    let compactions: Vec<Value> = events
        .iter()
        .filter(|e| e.kind == PrepEventKind::Compaction)
        .map(|e| {
            let after = calls
                .iter()
                .filter(|c| c.source == e.source && c.generation == e.generation && !c.is_sidechain)
                .filter(
                    |c| match (e.native_offset, c.native_offset, e.ts_ms, c.ts_ms) {
                        (Some(eo), Some(co), _, _) => co > eo,
                        (_, _, Some(et), Some(ct)) => ct > et,
                        _ => false,
                    },
                )
                .min_by_key(|c| {
                    (
                        (c.native_offset.is_none(), c.native_offset),
                        (c.ts_ms.is_none(), c.ts_ms),
                        (c.native_key.is_none(), c.native_key.clone()),
                        (c.msg_id.is_none(), c.msg_id.clone()),
                        (c.request_id.is_none(), c.request_id.clone()),
                    )
                });
            let context = after.and_then(|c| {
                let cache_write = match (c.cw_1h, c.cw_5m) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
                };
                Some(c.input? + c.cache_read? + cache_write?)
            });
            let mut row = serde_json::to_value(e).unwrap();
            row["first_context_after"] = serde_json::json!(context);
            row
        })
        .collect();

    let sorted = |rows: Vec<Value>| {
        let mut rows: Vec<String> = rows.iter().map(|r| strip_nulls(r).to_string()).collect();
        rows.sort();
        rows
    };
    BTreeMap::from([
        ("sources_v", sorted(sources)),
        ("sessions_v", sorted(sessions)),
        (
            "calls_v",
            sorted(
                calls
                    .iter()
                    .map(|c| json_without(c, &["sighting"]))
                    .collect(),
            ),
        ),
        (
            "turns_v",
            sorted(
                turns
                    .iter()
                    .map(|r| serde_json::to_value(r).unwrap())
                    .collect(),
            ),
        ),
        (
            "triggers_v",
            sorted(
                triggers
                    .iter()
                    .map(|r| serde_json::to_value(r).unwrap())
                    .collect(),
            ),
        ),
        (
            "events_v",
            sorted(
                events
                    .iter()
                    .map(|r| serde_json::to_value(r).unwrap())
                    .collect(),
            ),
        ),
        ("tool_uses_v", sorted(tool_uses)),
        ("compactions_v", sorted(compactions)),
    ])
}

/// Null and absent are the same Parquet value.
fn strip_nulls(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), strip_nulls(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn file_bytes(target: &Path, parts: &[String]) -> BTreeMap<String, Vec<u8>> {
    parts
        .iter()
        .map(|p| (p.clone(), fs::read(target.join(p)).expect("committed part")))
        .collect()
}

fn parquet_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            parquet_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "parquet") {
            out.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn publishes_every_table_with_schema_metadata_and_views_over_committed_parts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ParquetPrepStore::open(dir.path().to_path_buf()).unwrap();
    let (run1, sources) = &runs()[0];
    let rows = PrepRows {
        triggers: Vec::new(),
        ..run1.clone()
    };
    commit_run(&store, 1, &rows, sources);
    let state = published_state(dir.path());

    // Every fact table has a committed part, including an empty one for a
    // table this run produced no rows for; tool_uses rows are published.
    for table in FACT_TABLES {
        assert!(
            state
                .parts
                .iter()
                .any(|p| p.starts_with(&format!("tables/{table}/"))),
            "{table} has a part"
        );
    }
    let tool_rows: Vec<PrepToolUseRow> =
        read_rows(&dir.path().join("tables/tool_uses/run-000001.parquet"));
    assert_eq!(tool_rows, rows.tool_uses);
    let trigger_rows: Vec<PrepTriggerRow> =
        read_rows(&dir.path().join("tables/triggers/run-000001.parquet"));
    assert!(trigger_rows.is_empty());

    let mut files = Vec::new();
    parquet_files(&dir.path().join("tables"), &mut files);
    assert_eq!(files.len(), FACT_TABLES.len() + 2);
    for file in &files {
        assert_eq!(
            kv(file, META_SCHEMA_VERSION).as_deref(),
            Some(PREP_TABLE_SCHEMA_VERSION.to_string().as_str()),
            "{}",
            file.display()
        );
        let table = kv(file, META_TABLE).expect("table metadata");
        assert!(file.to_string_lossy().contains(&table));
    }

    // Snapshots project PrepSourceMeta, not path knowledge.
    let sources: Vec<Value> = read_rows(&dir.path().join("tables/sources.parquet"));
    assert_eq!(sources.len(), 2);
    assert!(
        sources
            .iter()
            .all(|s| s["project"] == "p" && s["is_sub"] == false)
    );

    // views.sql defines every canonical view over exactly the committed parts.
    let sql = fs::read_to_string(dir.path().join("views.sql")).unwrap();
    for view in VIEWS {
        assert!(
            sql.contains(&format!("CREATE OR REPLACE VIEW {view} AS")),
            "{view}"
        );
    }
    let named: BTreeSet<&str> = sql
        .split('\'')
        .filter(|s| s.starts_with("tables/") && s.matches('/').count() == 2)
        .collect();
    let committed: BTreeSet<&str> = state.parts.iter().map(String::as_str).collect();
    assert_eq!(named, committed);
}

#[test]
fn second_writer_is_refused_while_the_first_holds_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let first = ParquetPrepStore::open(dir.path().to_path_buf()).unwrap();
    let refused = ParquetPrepStore::open(dir.path().to_path_buf())
        .err()
        .expect("refused");
    assert_eq!(refused.kind(), PipelineErrorKind::Write);
    drop(first);
    ParquetPrepStore::open(dir.path().to_path_buf()).expect("released on drop");
}

/// Where a failure is injected: a directory squatting on the temporary path
/// the store writes next makes that write fail.
#[derive(Debug, Clone, Copy)]
enum Fault {
    /// Second fact part of the run (calls is already written).
    Part,
    /// Second snapshot (sources is already written).
    Snapshot,
    /// The state.json rename.
    State,
}

impl Fault {
    fn path(self, target: &Path) -> PathBuf {
        target.join(match self {
            Self::Part => "tables/turns/run-000002.parquet.tmp",
            Self::Snapshot => "tables/sessions.parquet.tmp",
            Self::State => "state.json.tmp",
        })
    }
}

#[test]
fn failure_before_the_state_rename_keeps_the_previous_publication() {
    for fault in [Fault::Part, Fault::Snapshot, Fault::State] {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path();
        let store = ParquetPrepStore::open(target.to_path_buf()).unwrap();
        let all = runs();
        commit_run(&store, 1, &all[0].0, &all[0].1);
        let published = views(target);
        let state_bytes = fs::read(target.join("state.json")).unwrap();
        let state = published_state(target);
        let parts = file_bytes(target, &state.parts);
        let sources_bytes = fs::read(target.join("tables/sources.parquet")).unwrap();

        fs::create_dir_all(fault.path(target)).unwrap();
        let (run2, sources2) = &all[1];
        assert!(
            store
                .commit(run2, &next_state(&store, 2, sources2))
                .is_err(),
            "{fault:?}"
        );

        // The publication is untouched: same state, same committed parts.
        assert_eq!(
            fs::read(target.join("state.json")).unwrap(),
            state_bytes,
            "{fault:?}"
        );
        assert_eq!(file_bytes(target, &state.parts), parts, "{fault:?}");
        let calls_part = target.join("tables/calls/run-000002.parquet");
        assert!(calls_part.exists(), "{fault:?}: parts are written first");
        let sources_now = fs::read(target.join("tables/sources.parquet")).unwrap();
        match fault {
            // Parts come before snapshots.
            Fault::Part => assert_eq!(sources_now, sources_bytes),
            // Snapshots come before state.
            Fault::Snapshot | Fault::State => assert_ne!(sources_now, sources_bytes),
        }

        // Recovery: load restores the snapshots and drops the orphans.
        fs::remove_dir(fault.path(target)).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.state.as_ref(), Some(&state));
        assert!(loaded.orphans_removed >= 1, "{fault:?}");
        assert!(!calls_part.exists());
        assert_eq!(views(target), published, "{fault:?}");
        assert_eq!(
            store.load().unwrap().orphans_removed,
            0,
            "recovery is idempotent"
        );

        // The retried run publishes once, without duplicates.
        commit_run(&store, 2, run2, sources2);
        let memory = MemoryPrepStore::new();
        commit_run(&memory, 1, &all[0].0, &all[0].1);
        commit_run(&memory, 2, run2, sources2);
        assert_eq!(
            views(target)["calls_v"],
            canonical_calls(&memory),
            "{fault:?}"
        );
    }
}

/// calls_v as the sealed in-memory reference store computes it (id-less calls
/// stay separate rows in both).
fn canonical_calls(memory: &MemoryPrepStore) -> Vec<String> {
    let mut rows: Vec<String> = memory
        .canonical()
        .calls
        .iter()
        .map(|c| strip_nulls(&json_without(c, &["sighting"])).to_string())
        .collect();
    rows.sort();
    rows
}

#[test]
fn compaction_keeps_every_view_and_drops_superseded_rows() {
    let compacted = tempfile::tempdir().unwrap();
    let twin = tempfile::tempdir().unwrap();
    let store = ParquetPrepStore::open(compacted.path().to_path_buf()).unwrap();
    let reference = ParquetPrepStore::open(twin.path().to_path_buf()).unwrap();
    let all = runs();
    for (run, (rows, sources)) in all.iter().enumerate().take(3) {
        commit_run(&store, run as u64 + 1, rows, sources);
        commit_run(&reference, run as u64 + 1, rows, sources);
    }
    let before = views(compacted.path());
    let old_parts = published_state(compacted.path()).parts;

    let report = store.compact().unwrap();
    assert_eq!(report.target, compacted.path());
    assert_eq!(report.parts_before, old_parts.len() as u64);
    assert_eq!(report.parts_after, FACT_TABLES.len() as u64);
    // Superseded generation of B dropped; A's repeated call sightings merged.
    assert!(report.rows_after.calls < report.rows_before.calls);
    assert!(report.rows_after.turns < report.rows_before.turns);
    assert!(report.rows_after.tool_uses < report.rows_before.tool_uses);
    assert_eq!(
        views(compacted.path()),
        before,
        "compaction changes no view"
    );
    for part in &old_parts {
        assert!(!compacted.path().join(part).exists(), "{part} deleted");
    }
    let state = published_state(compacted.path());
    let calls: Vec<PrepCallRow> = state
        .parts
        .iter()
        .filter(|p| p.starts_with("tables/calls/"))
        .flat_map(|p| read_rows::<PrepCallRow>(&compacted.path().join(p)))
        .collect();
    assert!(calls.iter().all(|c| c.source != B || c.generation == 2));
    assert!(calls.iter().all(|c| c.sighting == CallSighting::First));

    // A second compaction without an intervening run is stable.
    store.compact().unwrap();
    assert_eq!(views(compacted.path()), before);

    // Later sightings of compacted calls and tool uses merge exactly as they
    // would have without compaction.
    let (run4, sources4) = &all[3];
    commit_run(&store, 4, run4, sources4);
    commit_run(&reference, 4, run4, sources4);
    let after = views(compacted.path());
    assert_eq!(after, views(twin.path()));
    assert_ne!(after["calls_v"], before["calls_v"]);

    // The merged calls equal the sealed in-memory reference store's canonical rows.
    let memory = MemoryPrepStore::new();
    for (run, (rows, sources)) in all.iter().enumerate() {
        commit_run(&memory, run as u64 + 1, rows, sources);
    }
    assert_eq!(after["calls_v"], canonical_calls(&memory));
}

#[test]
fn compaction_view_rules_are_observable() {
    let dir = tempfile::tempdir().unwrap();
    let store = ParquetPrepStore::open(dir.path().to_path_buf()).unwrap();
    let all = runs();
    for (run, (rows, sources)) in all.iter().enumerate() {
        commit_run(&store, run as u64 + 1, rows, sources);
    }
    store.compact().unwrap();
    let views = views(dir.path());
    let calls: Vec<Value> = views["calls_v"]
        .iter()
        .map(|r| serde_json::from_str(r).unwrap())
        .collect();
    let m1 = calls
        .iter()
        .find(|c| c["source"] == A && c["msg_id"] == "m1")
        .unwrap();
    // First sighting's placement; counter maxima; last non-null stop reason.
    assert_eq!(m1["native_offset"], 0);
    assert_eq!(m1["input"], 12);
    assert_eq!(m1["output"], 30);
    assert_eq!(m1["stop_reason"], "max_tokens");
    assert_eq!(m1["records"], 4);
    // Calls without ids are never merged.
    assert_eq!(
        calls
            .iter()
            .filter(|c| c["source"] == A && c.get("msg_id").is_none())
            .count(),
        2
    );
    // Use and result sightings merge; the first result wins; id-less stay apart.
    let tools: Vec<Value> = views["tool_uses_v"]
        .iter()
        .map(|r| serde_json::from_str(r).unwrap())
        .collect();
    let t1 = tools.iter().find(|t| t["tool_use_id"] == "t1").unwrap();
    assert_eq!(
        (t1["name"].clone(), t1["result_bytes"].clone()),
        ("Bash".into(), 42.into())
    );
    assert_eq!(t1["duration_ms"], 7);
    assert_eq!(t1["native_offset"], 110);
    assert_eq!(
        tools
            .iter()
            .filter(|t| t.get("tool_use_id").is_none())
            .count(),
        2
    );
    let t9 = tools.iter().find(|t| t["tool_use_id"] == "t9").unwrap();
    assert_eq!(t9["generation"], 2);
    // first_context_after: next main-chain call after the boundary (the
    // sidechain m2 at offset 200 is skipped), by offset or else timestamp.
    let compactions: Vec<Value> = views["compactions_v"]
        .iter()
        .map(|r| serde_json::from_str(r).unwrap())
        .collect();
    let a = compactions.iter().find(|c| c["source"] == A).unwrap();
    assert_eq!(a["first_context_after"], 10 + 50 + 3);
    let b = compactions.iter().find(|c| c["source"] == B).unwrap();
    assert!(
        b.get("first_context_after").is_none(),
        "B generation 2 has no later call"
    );
}

#[test]
fn failed_compaction_leaves_orphans_that_load_removes() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path();
    let store = ParquetPrepStore::open(target.to_path_buf()).unwrap();
    let all = runs();
    for (run, (rows, sources)) in all.iter().enumerate().take(2) {
        commit_run(&store, run as u64 + 1, rows, sources);
    }
    let before = views(target);
    let state = published_state(target);
    let parts = file_bytes(target, &state.parts);

    fs::create_dir(Fault::State.path(target)).unwrap();
    assert!(store.compact().is_err());
    assert_eq!(published_state(target), state);
    assert_eq!(file_bytes(target, &state.parts), parts);
    assert!(
        target
            .join("tables/calls/compact-000002-0.parquet")
            .exists()
    );
    assert_eq!(views(target), before);

    fs::remove_dir(Fault::State.path(target)).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.orphans_removed, FACT_TABLES.len() as u64);
    assert!(
        !target
            .join("tables/calls/compact-000002-0.parquet")
            .exists()
    );

    // A crash after publishing a compaction but before deleting superseded
    // parts leaves unreferenced parts; load removes them.
    let old = target.join("tables/calls/run-000001.parquet");
    let old_bytes = fs::read(&old).unwrap();
    store.compact().unwrap();
    fs::write(&old, old_bytes).unwrap();
    assert_eq!(store.load().unwrap().orphans_removed, 1);
    assert!(!old.exists());
    assert_eq!(views(target), before);
}

#[test]
fn load_on_an_unpublished_target_removes_every_leftover() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path();
    let store = ParquetPrepStore::open(target.to_path_buf()).unwrap();
    fs::create_dir(Fault::State.path(target)).unwrap();
    let (run1, sources) = &runs()[0];
    assert!(store.commit(run1, &next_state(&store, 1, sources)).is_err());
    fs::remove_dir(Fault::State.path(target)).unwrap();

    let loaded = store.load().unwrap();
    assert_eq!(loaded.state, None);
    assert_eq!(loaded.orphans_removed, FACT_TABLES.len() as u64 + 2);
    let mut files = Vec::new();
    parquet_files(&target.join("tables"), &mut files);
    assert!(files.is_empty());
    assert!(store.compact().unwrap().parts_after == 0);
}

#[test]
fn a_part_name_already_committed_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = ParquetPrepStore::open(dir.path().to_path_buf()).unwrap();
    let (run1, sources) = &runs()[0];
    commit_run(&store, 1, run1, sources);
    let state = published_state(dir.path());
    let error = store
        .commit(run1, &next_state(&store, 1, sources))
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
    assert_eq!(published_state(dir.path()), state);
}

#[cfg(unix)]
#[test]
fn only_the_stores_own_files_are_written_or_removed_and_writes_never_follow_links() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = dir.path();
    let store = ParquetPrepStore::open(target.to_path_buf()).unwrap();
    let (run1, sources) = &runs()[0];
    commit_run(&store, 1, run1, sources);

    // Files a user keeps in the target, plus a crashed run's own leftovers.
    let foreign = [
        target.join("notes.txt"),
        target.join("tables/readme.md"),
        target.join("tables/calls/keep-me.parquet"),
        target.join("tables/calls/run-1.parquet"),
    ];
    for path in &foreign {
        fs::write(path, b"user data").unwrap();
    }
    let orphans = [
        target.join("tables/calls/run-999999.parquet"),
        target.join("tables/calls/compact-000009-0.parquet.tmp"),
        target.join("tables/sources.parquet.tmp"),
        target.join("state.json.tmp"),
    ];
    for path in &orphans {
        fs::write(path, b"leftover").unwrap();
    }
    assert_eq!(store.load().unwrap().orphans_removed, orphans.len() as u64);
    for path in &foreign {
        assert_eq!(fs::read(path).unwrap(), b"user data", "{}", path.display());
    }
    for path in &orphans {
        assert!(!path.exists(), "{}", path.display());
    }

    // Links planted at the store's temporary names are replaced, never followed.
    let victims: Vec<PathBuf> = ["a", "b", "c"]
        .iter()
        .map(|name| outside.path().join(name))
        .collect();
    for victim in &victims {
        fs::write(victim, b"outside").unwrap();
    }
    symlink(&victims[0], target.join("state.json.tmp")).unwrap();
    symlink(&victims[1], target.join("tables/sessions.parquet.tmp")).unwrap();
    symlink(
        &victims[2],
        target.join("tables/calls/run-000002.parquet.tmp"),
    )
    .unwrap();
    let (run2, sources2) = &runs()[1];
    commit_run(&store, 2, run2, sources2);
    for victim in &victims {
        assert_eq!(fs::read(victim).unwrap(), b"outside");
    }

    // Everything the store writes is owner-only.
    let state = published_state(target);
    let mut written = vec![
        target.join("state.json"),
        target.join("views.sql"),
        target.join("tables/sources.parquet"),
        target.join("tables/sessions.parquet"),
    ];
    written.extend(state.parts.iter().map(|part| target.join(part)));
    for path in &written {
        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{}", path.display());
    }
}
