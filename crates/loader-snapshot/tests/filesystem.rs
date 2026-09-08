use unisphere_core::{
    PipelineErrorKind, SnapshotFormat, SnapshotLimits, SnapshotLoader, SnapshotRef,
};
use unisphere_loader_snapshot::FileSnapshotLoader;

#[test]
fn invalid_inputs_fail_before_any_filesystem_operation() {
    let mut source = SnapshotRef {
        path: "relative.json".into(),
        format: SnapshotFormat::JsonDocument,
        session_id: None,
    };
    assert_eq!(
        FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidInput
    );
    let directory = tempfile::tempdir().unwrap();
    source.path = directory.path().join("missing");
    for limits in [
        SnapshotLimits {
            max_records: 0,
            max_record_bytes: 1,
            max_snapshot_bytes: 1,
        },
        SnapshotLimits {
            max_records: 1,
            max_record_bytes: 0,
            max_snapshot_bytes: 1,
        },
        SnapshotLimits {
            max_records: 1,
            max_record_bytes: 2,
            max_snapshot_bytes: 1,
        },
    ] {
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, limits)
                .unwrap_err()
                .kind(),
            PipelineErrorKind::InvalidInput
        );
    }
    source.format = SnapshotFormat::SqliteKeyValue {
        table: "ItemTable;DROP TABLE ItemTable".into(),
    };
    assert_eq!(
        FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidInput
    );
    assert!(!source.path.exists());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{ffi::CString, fs, os::unix::fs::symlink, sync::mpsc, time::Duration};
    use tempfile::TempDir;

    fn fixture(bytes: &[u8], format: SnapshotFormat) -> (TempDir, SnapshotRef) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native");
        fs::write(&path, bytes).unwrap();
        (
            directory,
            SnapshotRef {
                path,
                format,
                session_id: None,
            },
        )
    }

    fn limits(records: usize, record_bytes: usize, snapshot_bytes: usize) -> SnapshotLimits {
        SnapshotLimits {
            max_records: records,
            max_record_bytes: record_bytes,
            max_snapshot_bytes: snapshot_bytes,
        }
    }

    #[test]
    fn document_preserves_exact_bytes_and_content_revision_across_relocation_and_selection() {
        let raw = b" { \"native\" : [1, 2] }\r\n";
        let (_directory, mut source) = fixture(raw, SnapshotFormat::JsonDocument);
        let budget = limits(1, raw.len(), raw.len() + 8);
        let original = FileSnapshotLoader.read_snapshot(&source, budget).unwrap();
        assert_eq!(original.records[0].key, "document");
        assert_eq!(original.records[0].bytes, raw);
        let (_copy, other) = fixture(raw, SnapshotFormat::JsonDocument);
        source.session_id = Some("selected-only-by-mapper".into());
        assert_eq!(
            original.revision,
            FileSnapshotLoader
                .read_snapshot(&other, budget)
                .unwrap()
                .revision
        );
        assert_eq!(
            original.revision,
            FileSnapshotLoader
                .read_snapshot(&source, budget)
                .unwrap()
                .revision
        );
        fs::write(&source.path, b"{\"native\":[1,2]}").unwrap();
        assert_ne!(
            original.revision,
            FileSnapshotLoader
                .read_snapshot(&source, budget)
                .unwrap()
                .revision
        );
    }

    #[test]
    fn journal_preserves_order_cr_and_structural_locations_not_byte_offsets() {
        let (_directory, source) = fixture(b"{\"k\":1}\r\n[2]\n", SnapshotFormat::JsonJournal);
        let snapshot = FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap();
        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| (record.key.as_str(), record.bytes.as_slice()))
                .collect::<Vec<_>>(),
            vec![
                ("journal:0", b"{\"k\":1}\r".as_slice()),
                ("journal:1", b"[2]".as_slice())
            ]
        );
        fs::write(&source.path, b"[2]\n{\"k\":1}\r\n").unwrap();
        assert_ne!(
            snapshot.revision,
            FileSnapshotLoader
                .read_snapshot(&source, SnapshotLimits::default())
                .unwrap()
                .revision
        );
    }

    #[test]
    fn partial_journal_tail_never_publishes_prior_complete_records() {
        let (_directory, source) =
            fixture(b"{}\n{\"text\":\"\xf0\x9f", SnapshotFormat::JsonJournal);
        let failure = FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap_err();
        assert_eq!(failure.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(failure.offset(), None);
        fs::write(&source.path, "{}\n{\"text\":\"\u{1f680}\"}\n").unwrap();
        let complete = FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap();
        assert_eq!(
            complete.records[1].bytes,
            "{\"text\":\"\u{1f680}\"}".as_bytes()
        );
        // Even syntactically complete JSON is not a committed journal frame without LF.
        fs::write(&source.path, b"{}\n{}").unwrap();
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, SnapshotLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::InvalidData
        );
    }

    #[test]
    fn malformed_documents_and_blank_or_malformed_journal_records_fail_closed() {
        for raw in [b"".as_slice(), b"{", b"{} {}", b"\"\xff\""] {
            let (_directory, source) = fixture(raw, SnapshotFormat::JsonDocument);
            assert_eq!(
                FileSnapshotLoader
                    .read_snapshot(&source, SnapshotLimits::default())
                    .unwrap_err()
                    .kind(),
                PipelineErrorKind::InvalidData
            );
        }
        for raw in [
            b"\n".as_slice(),
            b"{}\n \r\n",
            b"{}\n{\n",
            b"{}\n\"\xff\"\n",
        ] {
            let (_directory, source) = fixture(raw, SnapshotFormat::JsonJournal);
            assert_eq!(
                FileSnapshotLoader
                    .read_snapshot(&source, SnapshotLimits::default())
                    .unwrap_err()
                    .kind(),
                PipelineErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn document_budgets_include_root_key_and_reject_overflow_without_partial_output() {
        let (_directory, source) = fixture(b"{}", SnapshotFormat::JsonDocument);
        let exact = FileSnapshotLoader
            .read_snapshot(&source, limits(1, 2, 10))
            .unwrap();
        assert_eq!(exact.records[0].bytes, b"{}");
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, limits(1, 1, 10))
                .unwrap_err()
                .kind(),
            PipelineErrorKind::RecordLimit
        );
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, limits(1, 2, 9))
                .unwrap_err()
                .kind(),
            PipelineErrorKind::BatchLimit
        );
        fs::write(&source.path, vec![b' '; 128 * 1024]).unwrap();
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, limits(1, 16, 32))
                .unwrap_err()
                .kind(),
            PipelineErrorKind::RecordLimit
        );
    }

    #[test]
    fn journal_count_value_and_aggregate_limits_reject_the_whole_snapshot() {
        let (_directory, source) = fixture(b"{}\n[1]\n", SnapshotFormat::JsonJournal);
        let exact = FileSnapshotLoader
            .read_snapshot(&source, limits(2, 3, 23))
            .unwrap();
        assert_eq!(exact.records[1].bytes, b"[1]");
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, limits(1, 3, 23))
                .unwrap_err()
                .kind(),
            PipelineErrorKind::BatchLimit
        );
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, limits(2, 2, 23))
                .unwrap_err()
                .kind(),
            PipelineErrorKind::RecordLimit
        );
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, limits(2, 3, 22))
                .unwrap_err()
                .kind(),
            PipelineErrorKind::BatchLimit
        );
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, limits(2, 3, 23))
                .unwrap(),
            exact
        );
    }

    #[test]
    fn empty_journal_has_a_stable_revision_and_deletions_change_it() {
        let (_directory, source) = fixture(b"{}\n", SnapshotFormat::JsonJournal);
        let populated = FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap();
        fs::write(&source.path, b"").unwrap();
        let empty = FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap();
        assert!(empty.records.is_empty());
        assert_ne!(populated.revision, empty.revision);
        assert_eq!(
            empty,
            FileSnapshotLoader
                .read_snapshot(&source, SnapshotLimits::default())
                .unwrap()
        );
    }

    #[test]
    fn final_symlinks_directories_and_missing_sources_are_never_followed_or_created() {
        for format in [
            SnapshotFormat::JsonDocument,
            SnapshotFormat::JsonJournal,
            SnapshotFormat::SqliteKeyValue {
                table: "ItemTable".into(),
            },
        ] {
            let (directory, mut source) = fixture(b"{}\n", format);
            let link = directory.path().join("link");
            symlink(&source.path, &link).unwrap();
            source.path = link;
            assert_eq!(
                FileSnapshotLoader
                    .read_snapshot(&source, SnapshotLimits::default())
                    .unwrap_err()
                    .kind(),
                PipelineErrorKind::Read
            );
            source.path = directory.path().to_owned();
            assert_eq!(
                FileSnapshotLoader
                    .read_snapshot(&source, SnapshotLimits::default())
                    .unwrap_err()
                    .kind(),
                PipelineErrorKind::Read
            );
            source.path = directory.path().join("missing");
            assert_eq!(
                FileSnapshotLoader
                    .read_snapshot(&source, SnapshotLimits::default())
                    .unwrap_err()
                    .kind(),
                PipelineErrorKind::Read
            );
            assert!(!source.path.exists());
        }
    }

    #[test]
    fn fifo_fails_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().unwrap();
        let source = SnapshotRef {
            path: directory.path().join("pipe"),
            format: SnapshotFormat::JsonDocument,
            session_id: None,
        };
        let native = CString::new(source.path.to_str().unwrap()).unwrap();
        // SAFETY: the live NUL-terminated path is valid; mkfifo retains no pointer.
        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _directory = directory;
            sender
                .send(FileSnapshotLoader.read_snapshot(&source, SnapshotLimits::default()))
                .unwrap();
        });
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("FIFO open blocked")
                .unwrap_err()
                .kind(),
            PipelineErrorKind::Read
        );
    }
}

#[cfg(not(unix))]
#[test]
fn valid_explicit_source_is_unsupported_off_unix() {
    let directory = tempfile::tempdir().unwrap();
    let source = SnapshotRef {
        path: directory.path().join("native"),
        format: SnapshotFormat::JsonDocument,
        session_id: None,
    };
    assert_eq!(
        FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::Unsupported
    );
}
