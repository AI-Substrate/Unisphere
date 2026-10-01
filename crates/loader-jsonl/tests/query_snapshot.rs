#![cfg(unix)]

use std::fs;
use tempfile::tempdir;
use unisphere_core::{PipelineErrorKind, ReadLimits, SessionLoader, SessionRef};
use unisphere_loader_jsonl::FileSessionLoader;

#[test]
fn query_snapshot_hashes_complete_physical_framing() {
    let temporary = tempdir().unwrap();
    let path = temporary.path().join("session.jsonl");
    let session = SessionRef { path: path.clone() };
    let loader = FileSessionLoader::new();
    let limits = ReadLimits {
        max_records: 8,
        max_record_bytes: 64,
        max_batch_bytes: 256,
    };

    fs::write(&path, b"\n{\"kind\":\"message\"}\n").unwrap();
    let first = loader.read_query_snapshot(&session, limits).unwrap();
    assert_eq!(first.input_bytes, 20);
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].bytes, b"{\"kind\":\"message\"}");

    fs::write(&path, b" \n{\"kind\":\"message\"}\n").unwrap();
    let second = loader.read_query_snapshot(&session, limits).unwrap();
    assert_ne!(first.revision, second.revision);
    assert_eq!(second.records.len(), 1);
}

#[test]
fn query_snapshot_rejects_partial_tail_without_changing_append_reader() {
    let temporary = tempdir().unwrap();
    let path = temporary.path().join("session.jsonl");
    fs::write(&path, b"{\"kind\":\"message\"}").unwrap();
    let session = SessionRef { path };
    let loader = FileSessionLoader::new();
    let limits = ReadLimits {
        max_records: 8,
        max_record_bytes: 64,
        max_batch_bytes: 256,
    };

    let failure = match loader.read_query_snapshot(&session, limits) {
        Ok(_) => panic!("partial query snapshot must fail"),
        Err(failure) => failure,
    };
    assert_eq!(failure.kind(), PipelineErrorKind::InvalidData);

    let batch = loader.read_batch(&session, None, limits).unwrap();
    assert!(batch.records.is_empty());
    assert!(batch.incomplete_tail);
    assert!(!batch.more);
}
