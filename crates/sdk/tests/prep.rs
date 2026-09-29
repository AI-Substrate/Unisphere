//! Incremental prep engine semantics over the in-memory store, loader and
//! recording fold: statuses, generations, crash recovery, live tails, coverage,
//! record fetch and single-source parity.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use unisphere_sdk::{
    PipelineError, PipelineErrorKind, ReadCursor, ReadLimits, SnapshotLimits,
    prep::{
        NativeAddress, PREP_TABLE_SCHEMA_VERSION, PrepApi, PrepBatch, PrepBinding, PrepCommit,
        PrepCompactReport, PrepDiscovery, PrepLoaded, PrepLoader, PrepOptions, PrepReadLimits,
        PrepRecordRequest, PrepReplaceReason, PrepReport, PrepRequest, PrepRows, PrepSkipCounts,
        PrepSourceKind, PrepSourceSet, PrepSourceStat, PrepSourceStatus, PrepState, PrepStore,
        Preparer, fold_source,
    },
};
use unisphere_testkit::prep::{MemoryLoader, MemoryPrepStore, RecordingFold};

const HARNESS: &str = "memory";
const ROOT: &str = "/native/root";

fn limits() -> PrepReadLimits {
    PrepReadLimits {
        read: ReadLimits {
            max_records: 3,
            max_record_bytes: 64,
            max_batch_bytes: 64,
        },
        snapshot: SnapshotLimits::default(),
    }
}

fn set(label: &str, root: &str) -> PrepSourceSet {
    PrepSourceSet {
        harness: HARNESS.into(),
        label: label.into(),
        root: PathBuf::from(root),
    }
}

fn request(roots: Vec<PrepSourceSet>) -> PrepRequest {
    PrepRequest {
        target: PathBuf::from("/target"),
        roots,
        options: PrepOptions::default(),
        limits: limits(),
        threads: 4,
        modified_since_ns: None,
    }
}

fn binding(policy: &'static str, loader: Arc<dyn PrepLoader>) -> PrepBinding {
    PrepBinding {
        fold: Arc::new(RecordingFold::new(HARNESS, policy, loader.kind())),
        loader,
    }
}

struct Rig {
    loader: Arc<MemoryLoader>,
    store: Arc<MemoryPrepStore>,
    policy: &'static str,
}

impl Rig {
    fn new(kind: PrepSourceKind) -> Self {
        Self {
            loader: Arc::new(MemoryLoader::new(kind, ROOT)),
            store: Arc::new(MemoryPrepStore::new()),
            policy: "v1",
        }
    }

    fn preparer(&self) -> Preparer<Shared> {
        Preparer::new(
            vec![binding(self.policy, self.loader.clone())],
            Shared(self.store.clone()),
        )
    }

    fn prep(&self) -> PrepReport {
        self.preparer()
            .prep(&request(vec![set("default", ROOT)]))
            .expect("prep succeeds")
    }
}

/// A store handle the test keeps while a [`Preparer`] owns another.
struct Shared(Arc<MemoryPrepStore>);

impl PrepStore for Shared {
    fn state(&self) -> Result<Option<PrepState>, PipelineError> {
        self.0.state()
    }
    fn load(&self) -> Result<PrepLoaded, PipelineError> {
        self.0.load()
    }
    fn commit(&self, rows: &PrepRows, state: &PrepState) -> Result<PrepCommit, PipelineError> {
        self.0.commit(rows, state)
    }
    fn compact(&self) -> Result<PrepCompactReport, PipelineError> {
        self.0.compact()
    }
}

fn key(file: &str) -> String {
    format!("{HARNESS}/default/{file}")
}

fn status_of(report: &PrepReport, file: &str) -> PrepSourceStatus {
    report
        .sources
        .iter()
        .find(|o| o.source == key(file))
        .map(|o| o.status)
        .expect("source reported")
}

/// Canonical call identities: (source, generation, offset or key, record text).
fn canonical_calls(store: &MemoryPrepStore) -> Vec<(String, u32, String)> {
    store
        .canonical()
        .calls
        .into_iter()
        .map(|c| {
            (
                c.source,
                c.generation,
                c.msg_id.expect("recording fold sets msg_id"),
            )
        })
        .collect()
}

fn texts(store: &MemoryPrepStore, file: &str) -> Vec<String> {
    canonical_calls(store)
        .into_iter()
        .filter(|(source, _, _)| *source == key(file))
        .map(|(_, _, text)| text)
        .collect()
}

#[test]
fn unchanged_rerun_reads_nothing_and_commits_nothing() {
    let rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("p/a.jsonl", b"one\ntwo\n");
    let first = rig.prep();
    assert_eq!(status_of(&first, "p/a.jsonl"), PrepSourceStatus::New);
    let (reads, commits) = (rig.loader.reads(), rig.store.commits());

    let second = rig.prep();
    assert_eq!(rig.loader.reads(), reads, "no body read");
    assert_eq!(rig.store.commits(), commits, "no commit");
    assert_eq!(second.bytes_read, 0);
    assert_eq!(second.rows_written, Default::default());
    assert!(
        second.sources.is_empty(),
        "unchanged is counted, not listed"
    );
    assert_eq!(second.sets[0].by_status.get("unchanged"), Some(&1));
    assert_eq!(second.run, first.run);
}

#[test]
fn append_contributes_only_records_after_the_cursor() {
    let rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\ntwo\n");
    rig.prep();
    rig.loader.append("a.jsonl", b"three\n");
    let report = rig.prep();
    assert_eq!(status_of(&report, "a.jsonl"), PrepSourceStatus::Appended);
    assert_eq!(report.bytes_read, 6);
    assert_eq!(report.rows_written.calls, 1);
    let outcome = &report.sources[0];
    assert_eq!((outcome.generation, outcome.committed_offset), (0, 14));
    assert_eq!(texts(&rig.store, "a.jsonl"), ["one", "two", "three"]);
    assert_eq!(rig.store.all_rows().calls.len(), 3, "nothing re-emitted");
}

#[test]
fn replaced_sources_start_a_new_generation_and_supersede_old_rows() {
    type Change = fn(&MemoryLoader);
    let cases: [(&str, Change, PrepReplaceReason, &[&str]); 3] = [
        (
            "rotate",
            |l| l.rotate("a.jsonl", b"x\n"),
            PrepReplaceReason::Rotated,
            &["x"],
        ),
        (
            "truncate",
            |l| l.truncate("a.jsonl", 4),
            PrepReplaceReason::Truncated,
            &["one"],
        ),
        (
            "same-size rewrite with new mtime",
            |l| l.rewrite("a.jsonl", b"ONE\nTWO\n", true),
            PrepReplaceReason::Rewritten,
            &["ONE", "TWO"],
        ),
    ];
    for (name, change, reason, expected) in cases {
        let rig = Rig::new(PrepSourceKind::Append);
        rig.loader.append("a.jsonl", b"one\ntwo\n");
        rig.prep();
        change(&rig.loader);
        let report = rig.prep();
        assert_eq!(
            status_of(&report, "a.jsonl"),
            PrepSourceStatus::Replaced { reason },
            "{name}"
        );
        assert_eq!(report.sources[0].generation, 1, "{name}");
        assert_eq!(texts(&rig.store, "a.jsonl"), expected, "{name}");
        assert!(
            canonical_calls(&rig.store).iter().all(|(_, g, _)| *g == 1),
            "{name}: no superseded rows in the canonical view"
        );
    }
}

#[test]
fn same_size_same_mtime_rewrite_is_caught_by_the_anchor_at_next_growth() {
    let rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\ntwo\n");
    rig.prep();
    rig.loader.rewrite("a.jsonl", b"uno\ndos\n", false);
    let hidden = rig.prep();
    assert!(hidden.sources.is_empty(), "invisible to stat");
    rig.loader.append("a.jsonl", b"tres\n");
    let report = rig.prep();
    assert_eq!(
        status_of(&report, "a.jsonl"),
        PrepSourceStatus::Replaced {
            reason: PrepReplaceReason::Rewritten
        }
    );
    assert_eq!(texts(&rig.store, "a.jsonl"), ["uno", "dos", "tres"]);
}

#[test]
fn touched_but_unchanged_append_source_is_unchanged() {
    let rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\n");
    rig.prep();
    rig.loader.rewrite("a.jsonl", b"one\n", true);
    let report = rig.prep();
    assert!(report.sources.is_empty());
    assert_eq!(report.rows_written, Default::default());
    let (reads, commits) = (rig.loader.reads(), rig.store.commits());
    rig.prep();
    assert_eq!(
        (rig.loader.reads(), rig.store.commits()),
        (reads, commits),
        "the observed stat was committed, so the next run is one stat"
    );
}

#[test]
fn policy_change_replaces_every_source_of_the_harness() {
    let mut rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\n");
    rig.loader.append("b.jsonl", b"two\n");
    rig.prep();
    rig.policy = "v2";
    let report = rig.prep();
    for file in ["a.jsonl", "b.jsonl"] {
        assert_eq!(
            status_of(&report, file),
            PrepSourceStatus::Replaced {
                reason: PrepReplaceReason::Policy
            }
        );
    }
    assert!(canonical_calls(&rig.store).iter().all(|(_, g, _)| *g == 1));
    assert_eq!(report.sets[0].policy.as_deref(), Some("v2"));
}

#[test]
fn table_schema_change_replaces_sources() {
    let rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\n");
    rig.prep();
    let mut old = rig.store.state().unwrap().unwrap();
    old.table_schema_version = PREP_TABLE_SCHEMA_VERSION - 1;
    rig.store.commit(&PrepRows::default(), &old).unwrap();
    let report = rig.prep();
    assert_eq!(
        status_of(&report, "a.jsonl"),
        PrepSourceStatus::Replaced {
            reason: PrepReplaceReason::Schema
        }
    );
    assert_eq!(report.sources[0].generation, 1);
    let state = rig.store.state().unwrap().unwrap();
    assert_eq!(state.table_schema_version, PREP_TABLE_SCHEMA_VERSION);
    assert_eq!(texts(&rig.store, "a.jsonl"), ["one"]);
}

#[test]
fn moved_root_replaces_sources_under_the_same_label() {
    let store = Arc::new(MemoryPrepStore::new());
    let old = Arc::new(MemoryLoader::new(PrepSourceKind::Append, "/old"));
    let new = Arc::new(MemoryLoader::new(PrepSourceKind::Append, "/new"));
    old.append("a.jsonl", b"one\n");
    new.append("a.jsonl", b"one\n");
    Preparer::new(vec![binding("v1", old)], Shared(store.clone()))
        .prep(&request(vec![set("default", "/old")]))
        .unwrap();
    let report = Preparer::new(vec![binding("v1", new)], Shared(store.clone()))
        .prep(&request(vec![set("default", "/new")]))
        .unwrap();
    assert_eq!(
        status_of(&report, "a.jsonl"),
        PrepSourceStatus::Replaced {
            reason: PrepReplaceReason::RootMoved
        }
    );
    let state = store.state().unwrap().unwrap();
    assert_eq!(state.sets["memory/default"].root, PathBuf::from("/new"));
    assert_eq!(
        state.sources[&key("a.jsonl")].path,
        PathBuf::from("/new/a.jsonl")
    );
}

#[test]
fn failed_commit_keeps_the_previous_state_and_recovery_has_no_duplicates() {
    let rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\ntwo\n");
    rig.prep();
    let committed = rig.store.state().unwrap();
    rig.loader.append("a.jsonl", b"three\n");
    rig.store.fail_next_commit();
    let error = rig
        .preparer()
        .prep(&request(vec![set("default", ROOT)]))
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::Write);
    assert_eq!(
        rig.store.state().unwrap(),
        committed,
        "previous publication"
    );

    rig.loader.append("a.jsonl", b"four\n");
    let report = rig.prep();
    assert_eq!(status_of(&report, "a.jsonl"), PrepSourceStatus::Appended);
    assert_eq!(
        texts(&rig.store, "a.jsonl"),
        ["one", "two", "three", "four"]
    );

    let fresh = Rig::new(PrepSourceKind::Append);
    fresh.loader.append("a.jsonl", b"one\ntwo\nthree\nfour\n");
    fresh.prep();
    assert_eq!(rig.store.canonical(), fresh.store.canonical());
}

#[test]
fn unreadable_source_keeps_its_committed_state_and_rows() {
    let rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\n");
    rig.prep();
    let commits = rig.store.commits();
    rig.loader.append("a.jsonl", b"two\n");
    rig.loader.set_unreadable("a.jsonl", true);
    rig.loader.append("new.jsonl", b"FAIL\n");
    let report = rig.prep();
    for file in ["a.jsonl", "new.jsonl"] {
        let outcome = report
            .sources
            .iter()
            .find(|o| o.source == key(file))
            .unwrap();
        assert_eq!(outcome.status, PrepSourceStatus::Unreadable, "{file}");
        assert!(outcome.error.is_some(), "{file}");
        assert_eq!(outcome.rows, 0);
    }
    assert_eq!(
        rig.store.commits(),
        commits,
        "failures alone commit nothing"
    );
    assert_eq!(texts(&rig.store, "a.jsonl"), ["one"]);

    rig.loader.set_unreadable("a.jsonl", false);
    let report = rig.prep();
    assert_eq!(status_of(&report, "a.jsonl"), PrepSourceStatus::Appended);
    assert_eq!(texts(&rig.store, "a.jsonl"), ["one", "two"]);
    assert!(
        !rig.store
            .state()
            .unwrap()
            .unwrap()
            .sources
            .contains_key(&key("new.jsonl"))
    );
}

#[test]
fn failure_during_a_replacement_never_reuses_a_superseded_generation() {
    let mut rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\n");
    rig.loader.append("b.jsonl", b"two\n");
    rig.prep();
    rig.policy = "v2";
    rig.loader.set_unreadable("a.jsonl", true);
    let failed = rig.prep();
    assert_eq!(status_of(&failed, "a.jsonl"), PrepSourceStatus::Unreadable);
    assert_eq!(texts(&rig.store, "a.jsonl"), ["one"], "previous rows kept");

    rig.loader.set_unreadable("a.jsonl", false);
    let report = rig.prep();
    assert_eq!(
        status_of(&report, "a.jsonl"),
        PrepSourceStatus::Replaced {
            reason: PrepReplaceReason::Policy
        }
    );
    assert_eq!(report.sources[0].generation, 1);
    assert_eq!(rig.store.canonical().calls.len(), 2);
}

#[test]
fn live_writer_never_fails_and_converges_to_a_fresh_run() {
    let rig = Rig::new(PrepSourceKind::Append);
    let files = ["live/a.jsonl", "live/b.jsonl", "live/c.jsonl"];
    let done = AtomicBool::new(false);
    let mut runs = 0u32;
    let written = thread::scope(|scope| {
        let writer = scope.spawn(|| {
            // Deterministic chunking that ignores line boundaries.
            let mut seed = 0x2545_f491_4f6c_dd1du64;
            let mut next = || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed
            };
            let mut written = vec![Vec::new(); files.len()];
            for n in 0..300 {
                let line = format!("record-{n:04}-{}\n", "x".repeat((next() % 40) as usize));
                let bytes = line.as_bytes();
                let split = (next() as usize) % bytes.len();
                rig.loader.append(files[n % files.len()], &bytes[..split]);
                thread::yield_now();
                rig.loader.append(files[n % files.len()], &bytes[split..]);
                written[n % files.len()].extend_from_slice(bytes);
            }
            // Leave a partial tail behind.
            rig.loader.append(files[0], b"tail");
            written[0].extend_from_slice(b"tail");
            done.store(true, Ordering::SeqCst);
            written
        });
        while !done.load(Ordering::SeqCst) {
            let report = rig
                .preparer()
                .prep(&request(vec![set("default", ROOT)]))
                .expect("a live source never fails the run");
            assert!(
                report
                    .sources
                    .iter()
                    .all(|o| o.status != PrepSourceStatus::Unreadable),
                "{report:?}"
            );
            runs += 1;
        }
        writer.join().unwrap()
    });
    assert!(runs > 0);
    let last = rig.prep();
    assert_eq!(
        last.pending_tail_bytes, 4,
        "partial tail reported, not read"
    );

    let fresh = Rig::new(PrepSourceKind::Append);
    for (file, bytes) in files.iter().zip(&written) {
        fresh.loader.append(file, bytes);
    }
    fresh.prep();
    // Commit order interleaves sources across runs; content must match exactly.
    let by_position = |store: &MemoryPrepStore| {
        let mut calls = store.canonical().calls;
        calls.sort_by(|a, b| (&a.source, a.native_offset).cmp(&(&b.source, b.native_offset)));
        calls
    };
    assert_eq!(by_position(&rig.store), by_position(&fresh.store));
    assert_eq!(rig.store.canonical().calls.len(), 300);
    assert!(
        canonical_calls(&rig.store).iter().all(|(_, g, _)| *g == 0),
        "appends never replace"
    );

    rig.loader.append(files[0], b"\n");
    let report = rig.prep();
    assert_eq!(status_of(&report, files[0]), PrepSourceStatus::Appended);
    assert_eq!(report.pending_tail_bytes, 0);
    assert_eq!(texts(&rig.store, files[0]).last().unwrap(), "tail");
}

// ---------------------------------------------------------------------------
// Coverage
// ---------------------------------------------------------------------------

/// Routes calls to one memory loader per root and counts every call.
struct Routed {
    loaders: Vec<Arc<MemoryLoader>>,
    calls: AtomicU64,
}

impl Routed {
    fn new(loaders: Vec<Arc<MemoryLoader>>) -> Self {
        Self {
            loaders,
            calls: AtomicU64::new(0),
        }
    }

    fn route(&self, path: &Path) -> Result<&MemoryLoader, PipelineError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.loaders
            .iter()
            .find(|l| path.starts_with(l.root()))
            .map(AsRef::as_ref)
            .ok_or_else(|| PipelineError::new(PipelineErrorKind::Read, None))
    }
}

impl PrepLoader for Routed {
    fn kind(&self) -> PrepSourceKind {
        self.loaders[0].kind()
    }
    fn discover(
        &self,
        root: &Path,
        accept: &dyn Fn(&str) -> bool,
    ) -> Result<PrepDiscovery, PipelineError> {
        self.route(root)?.discover(root, accept)
    }
    fn stat(&self, root: &Path, path: &Path) -> Result<PrepSourceStat, PipelineError> {
        self.route(root)?.stat(root, path)
    }
    fn read(
        &self,
        stat: &PrepSourceStat,
        from: Option<&ReadCursor>,
        limits: PrepReadLimits,
    ) -> Result<PrepBatch, PipelineError> {
        self.route(&stat.path)?.read(stat, from, limits)
    }
    fn anchor(&self, stat: &PrepSourceStat, offset: u64) -> Result<String, PipelineError> {
        self.route(&stat.path)?.anchor(stat, offset)
    }
    fn record_at(
        &self,
        path: &Path,
        address: &NativeAddress,
        max_bytes: usize,
    ) -> Result<Vec<u8>, PipelineError> {
        self.route(path)?.record_at(path, address, max_bytes)
    }
}

#[test]
fn report_accounts_for_every_discovered_and_committed_source() {
    let main = Arc::new(MemoryLoader::new(PrepSourceKind::Append, "/home/.claude"));
    let alt = Arc::new(MemoryLoader::new(
        PrepSourceKind::Append,
        "/home/.claude-alt",
    ));
    main.append("a.jsonl", b"one\n");
    main.append("old.jsonl", b"old\n");
    main.append("gone.jsonl", b"gone\n");
    alt.append("a.jsonl", b"alt\n");
    alt.set_skipped(PrepSkipCounts {
        symlinks: 2,
        hidden: 3,
        unreadable_entries: 1,
    });
    let store = Arc::new(MemoryPrepStore::new());
    let preparer = Preparer::new(
        vec![binding(
            "v1",
            Arc::new(Routed::new(vec![main.clone(), alt.clone()])),
        )],
        Shared(store.clone()),
    );
    let roots = vec![
        set("default", "/home/.claude"),
        set("alt", "/home/.claude-alt"),
        PrepSourceSet {
            harness: "no-binding".into(),
            label: "default".into(),
            root: PathBuf::from("/elsewhere"),
        },
    ];
    let first = preparer.prep(&request(roots.clone())).unwrap();
    assert_eq!(first.sets[1].skipped.symlinks, 2);
    assert_eq!(first.sets[1].skipped.hidden, 3);
    assert_eq!(first.sets[1].skipped.unreadable_entries, 1);
    assert_eq!(first.sets[0].skipped, PrepSkipCounts::default());
    assert!(!first.sets[2].supported);
    assert_eq!(first.sets[2].by_status.get("unsupported"), Some(&1));
    let state = store.state().unwrap().unwrap();
    assert!(state.sources.contains_key("memory/default/a.jsonl"));
    assert!(
        state.sources.contains_key("memory/alt/a.jsonl"),
        "extra root keyed by label"
    );

    // Only a.jsonl is recent; old.jsonl is scoped out; gone.jsonl disappears.
    main.remove("gone.jsonl");
    main.append("a.jsonl", b"two\n");
    let since = main
        .stat(
            Path::new("/home/.claude"),
            Path::new("/home/.claude/a.jsonl"),
        )
        .unwrap()
        .mtime_ns;
    let mut scoped = request(roots);
    scoped.modified_since_ns = Some(since);
    let report = preparer.prep(&scoped).unwrap();
    let main_set = &report.sets[0];
    assert_eq!(main_set.discovered, 2);
    assert_eq!(main_set.by_status.get("appended"), Some(&1));
    assert_eq!(main_set.by_status.get("skipped"), Some(&1));
    assert_eq!(main_set.by_status.get("missing"), Some(&1));
    assert_eq!(report.sets[1].by_status.get("skipped"), Some(&1));
    let listed: Vec<(&str, &str)> = report
        .sources
        .iter()
        .map(|o| (o.source.as_str(), o.status.label()))
        .collect();
    assert_eq!(
        listed,
        [
            ("memory/default/a.jsonl", "appended"),
            ("memory/default/gone.jsonl", "missing"),
        ],
        "skipped sources are counted, not listed"
    );
    let state = store.state().unwrap().unwrap();
    for kept in ["old.jsonl", "gone.jsonl"] {
        assert!(
            state
                .sources
                .contains_key(&format!("memory/default/{kept}"))
        );
    }
    let canonical: Vec<String> = store
        .canonical()
        .calls
        .into_iter()
        .filter_map(|c| c.msg_id)
        .collect();
    for text in ["old", "gone", "alt", "two"] {
        assert!(canonical.iter().any(|t| t == text), "{text} kept");
    }
}

#[test]
fn duplicate_set_keys_are_refused() {
    let rig = Rig::new(PrepSourceKind::Append);
    let error = rig
        .preparer()
        .prep(&request(vec![set("default", ROOT), set("default", ROOT)]))
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
}

// ---------------------------------------------------------------------------
// Snapshot sources
// ---------------------------------------------------------------------------

#[test]
fn snapshot_sources_are_judged_by_revision() {
    let rig = Rig::new(PrepSourceKind::Snapshot);
    rig.loader
        .put_snapshot("s.json", &[("k1", b"one"), ("k2", b"two")]);
    let first = rig.prep();
    assert_eq!(status_of(&first, "s.json"), PrepSourceStatus::New);

    // Touched, same revision: unchanged, rows discarded, stat committed.
    rig.loader
        .put_snapshot("s.json", &[("k1", b"one"), ("k2", b"two")]);
    let touched = rig.prep();
    assert!(touched.sources.is_empty());
    assert_eq!(touched.rows_written, Default::default());
    assert_eq!(touched.pending_tail_bytes, 0);
    let reads = rig.loader.reads();
    rig.prep();
    assert_eq!(rig.loader.reads(), reads, "then one stat");

    rig.loader.put_snapshot("s.json", &[("k1", b"uno")]);
    let report = rig.prep();
    assert_eq!(
        status_of(&report, "s.json"),
        PrepSourceStatus::Replaced {
            reason: PrepReplaceReason::Revision
        }
    );
    assert_eq!(report.sources[0].generation, 1);
    let calls = rig.store.canonical().calls;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].native_key.as_deref(), Some("k1"));
    assert_eq!(calls[0].msg_id.as_deref(), Some("uno"));
}

// ---------------------------------------------------------------------------
// Record fetch
// ---------------------------------------------------------------------------

fn record_request(source: String, offset: u64, include_content: bool) -> PrepRecordRequest {
    PrepRecordRequest {
        target: PathBuf::from("/target"),
        source,
        address: NativeAddress {
            offset: Some(offset),
            key: None,
        },
        include_content,
        max_bytes: 1024,
    }
}

#[test]
fn record_returns_the_addressed_record_only_with_content_opt_in() {
    let loader = Arc::new(Routed::new(vec![Arc::new(MemoryLoader::new(
        PrepSourceKind::Append,
        ROOT,
    ))]));
    let memory = loader.loaders[0].clone();
    memory.append("a.jsonl", b"first\nsecond\npartial");
    let store = Arc::new(MemoryPrepStore::new());
    let preparer = Preparer::new(vec![binding("v1", loader.clone())], Shared(store.clone()));
    preparer.prep(&request(vec![set("default", ROOT)])).unwrap();
    let offset = store.canonical().calls[1].native_offset.unwrap();

    let calls = loader.calls.load(Ordering::SeqCst);
    let refused = preparer
        .record(&record_request(key("a.jsonl"), offset, false))
        .unwrap_err();
    assert_eq!(refused.kind(), PipelineErrorKind::InvalidInput);
    assert_eq!(loader.calls.load(Ordering::SeqCst), calls, "no loader call");

    let record = preparer
        .record(&record_request(key("a.jsonl"), offset, true))
        .unwrap();
    assert_eq!(record.bytes, b"second");
    assert_eq!(record.path, PathBuf::from(ROOT).join("a.jsonl"));

    for (offset, why) in [(2, "not a line start"), (13, "uncommitted tail")] {
        let error = preparer
            .record(&record_request(key("a.jsonl"), offset, true))
            .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidInput, "{why}");
    }

    memory.rotate("a.jsonl", b"other\nsecond\n");
    let error = preparer
        .record(&record_request(key("a.jsonl"), offset, true))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        PipelineErrorKind::SourceChanged,
        "not the committed file"
    );
}

// ---------------------------------------------------------------------------
// Single-source parity
// ---------------------------------------------------------------------------

#[test]
fn fold_source_without_a_store_matches_run_prep() {
    let rig = Rig::new(PrepSourceKind::Append);
    rig.loader.append("a.jsonl", b"one\ntwo\n");
    rig.prep();
    rig.loader.append("a.jsonl", b"three\nfour\nfi");
    rig.prep();
    let committed = rig.store.state().unwrap().unwrap().sources[&key("a.jsonl")].clone();

    let stat = rig
        .loader
        .stat(Path::new(ROOT), &Path::new(ROOT).join("a.jsonl"))
        .unwrap();
    let mut rows = PrepRows::default();
    let fold = fold_source(
        rig.loader.as_ref(),
        &RecordingFold::new(HARNESS, "v1", PrepSourceKind::Append),
        &stat,
        &key("a.jsonl"),
        0,
        None,
        PrepOptions::default(),
        limits(),
        &mut |batch| rows.extend(batch),
    )
    .unwrap();
    assert_eq!(fold.facts, committed.facts);
    assert_eq!(fold.checkpoint, committed.checkpoint);
    assert_eq!(fold.cursor.map(|c| c.offset), Some(committed.offset));
    assert_eq!(fold.anchor, committed.anchor);
    assert_eq!(fold.pending_tail_bytes, 2);
    assert_eq!(rows, rig.store.canonical());
}
