use unisphere_core::{PipelineErrorKind, ReadLimits, SessionLoader, SessionRef, SourceScope};
use unisphere_loader_jsonl::FileSessionLoader;

#[cfg(unix)]
mod unix {
    use std::{
        ffi::{CString, OsString},
        fs::{self, OpenOptions},
        io::Write,
        os::unix::{ffi::OsStringExt, fs::symlink},
        sync::mpsc,
        time::Duration,
    };

    use super::*;
    use tempfile::{TempDir, tempdir};
    use unisphere_core::SourceIdentity;
    use unisphere_testkit::collection::{CLAUDE_BASIC, CLAUDE_PARTS, fixture_records};

    fn fixture(bytes: &[u8]) -> (TempDir, SessionRef) {
        let directory = tempdir().unwrap();
        let session = SessionRef {
            path: directory.path().join("session.jsonl"),
        };
        fs::write(&session.path, bytes).unwrap();
        (directory, session)
    }

    fn limits(records: usize, record_bytes: usize, batch_bytes: usize) -> ReadLimits {
        ReadLimits {
            max_records: records,
            max_record_bytes: record_bytes,
            max_batch_bytes: batch_bytes,
        }
    }

    #[test]
    fn lists_only_immediate_regular_jsonl_files_in_sorted_order() {
        let directory = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("outside.jsonl"), b"outside\n").unwrap();
        fs::write(directory.path().join("z.jsonl"), b"z\n").unwrap();
        fs::write(directory.path().join("a.jsonl"), b"a\n").unwrap();
        fs::write(directory.path().join("sidecar.json"), b"ignored").unwrap();
        fs::create_dir(directory.path().join("nested.jsonl")).unwrap();
        fs::write(
            directory.path().join("nested.jsonl/hidden.jsonl"),
            b"hidden\n",
        )
        .unwrap();
        symlink(
            outside.path().join("outside.jsonl"),
            directory.path().join("link.jsonl"),
        )
        .unwrap();
        symlink(outside.path(), directory.path().join("linked-dir")).unwrap();
        let scope = SourceScope {
            root: directory.path().into(),
            max_sessions: 2,
        };
        let found = FileSessionLoader.list_sessions(&scope).unwrap();
        assert_eq!(
            found,
            vec![
                SessionRef {
                    path: directory.path().join("a.jsonl")
                },
                SessionRef {
                    path: directory.path().join("z.jsonl")
                },
            ]
        );
        assert_eq!(
            FileSessionLoader
                .list_sessions(&SourceScope {
                    max_sessions: 1,
                    ..scope
                })
                .unwrap_err()
                .kind(),
            PipelineErrorKind::ListingLimit,
        );
    }

    #[test]
    fn empty_leaf_is_valid_but_zero_listing_budget_is_not() {
        let directory = tempdir().unwrap();
        let scope = SourceScope {
            root: directory.path().into(),
            max_sessions: 1,
        };
        assert_eq!(FileSessionLoader.list_sessions(&scope).unwrap(), vec![]);
        let invalid = SourceScope {
            root: directory.path().join("missing"),
            max_sessions: 0,
        };
        assert_eq!(
            FileSessionLoader
                .list_sessions(&invalid)
                .unwrap_err()
                .kind(),
            PipelineErrorKind::InvalidInput
        );
    }

    #[test]
    fn listing_requires_an_explicit_directory_not_a_file_or_symlink() {
        let (directory, session) = fixture(b"a\n");
        let scope = SourceScope {
            root: session.path,
            max_sessions: 1,
        };
        assert_eq!(
            FileSessionLoader.list_sessions(&scope).unwrap_err().kind(),
            PipelineErrorKind::Read
        );
        let link = directory.path().join("directory-link");
        symlink(directory.path(), &link).unwrap();
        assert_eq!(
            FileSessionLoader
                .list_sessions(&SourceScope {
                    root: link,
                    max_sessions: 1
                })
                .unwrap_err()
                .kind(),
            PipelineErrorKind::Read
        );
    }

    #[test]
    fn non_utf8_roots_and_inputs_are_rejected_before_io() {
        let directory = tempdir().unwrap();
        let path = directory
            .path()
            .join(OsString::from_vec(b"bad-\xff.jsonl".to_vec()));
        assert_eq!(
            FileSessionLoader
                .read_batch(
                    &SessionRef { path: path.clone() },
                    None,
                    ReadLimits::default()
                )
                .unwrap_err()
                .kind(),
            PipelineErrorKind::InvalidInput
        );
        assert_eq!(
            FileSessionLoader
                .list_sessions(&SourceScope {
                    root: path,
                    max_sessions: 1
                })
                .unwrap_err()
                .kind(),
            PipelineErrorKind::InvalidInput
        );
    }

    #[test]
    fn non_utf8_candidates_are_rejected_when_filesystem_permits_them() {
        let directory = tempdir().unwrap();
        let path = directory
            .path()
            .join(OsString::from_vec(b"bad-\xff.jsonl".to_vec()));
        match fs::write(&path, b"a\n") {
            Ok(()) => assert_eq!(
                FileSessionLoader
                    .list_sessions(&SourceScope {
                        root: directory.path().into(),
                        max_sessions: 1
                    })
                    .unwrap_err()
                    .kind(),
                PipelineErrorKind::InvalidInput,
            ),
            Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {
                eprintln!(
                    "NOT EXERCISED: non-UTF-8 candidate rejection; filesystem refused fixture pathname: {error}"
                );
            }
            Err(error) => panic!("could not create non-UTF-8 candidate fixture: {error}"),
        }
    }

    #[test]
    fn relative_inputs_and_invalid_limits_fail_before_opening() {
        let session = SessionRef {
            path: "not-opened.jsonl".into(),
        };
        assert_eq!(
            FileSessionLoader
                .read_batch(&session, None, ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::InvalidInput
        );
        assert_eq!(
            FileSessionLoader
                .list_sessions(&SourceScope {
                    root: ".".into(),
                    max_sessions: 1
                })
                .unwrap_err()
                .kind(),
            PipelineErrorKind::InvalidInput
        );
        let directory = tempdir().unwrap();
        let missing = SessionRef {
            path: directory.path().join("missing"),
        };
        for invalid in [
            limits(0, 1, 1),
            limits(1, 0, 1),
            limits(1, 1, 0),
            limits(1, 2, 1),
        ] {
            assert_eq!(
                FileSessionLoader
                    .read_batch(&missing, None, invalid)
                    .unwrap_err()
                    .kind(),
                PipelineErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn preserves_native_bytes_and_shared_fixture_offsets_without_parsing() {
        for bytes in [CLAUDE_BASIC, CLAUDE_PARTS, b"not JSON\r\n\xff\n\t\r\n"] {
            let (_directory, session) = fixture(bytes);
            let batch = FileSessionLoader
                .read_batch(&session, None, ReadLimits::default())
                .unwrap();
            assert_eq!(batch.records, fixture_records(bytes));
            assert_eq!(batch.next_cursor.offset, bytes.len() as u64);
            assert!(!batch.more);
            assert!(!batch.incomplete_tail);
            batch.validate(ReadLimits::default()).unwrap();
        }
    }

    #[test]
    fn physical_blank_lines_consume_record_budget_and_resume_progress() {
        let (_directory, session) = fixture(b" \n\t\nvalue\n");
        let limits = limits(2, 8, 16);
        let blank = FileSessionLoader
            .read_batch(&session, None, limits)
            .unwrap();
        assert_eq!(blank.records, vec![]);
        assert_eq!(blank.next_cursor.offset, 4);
        assert!(blank.more);
        let value = FileSessionLoader
            .read_batch(&session, Some(&blank.next_cursor), limits)
            .unwrap();
        assert_eq!(value.records, fixture_records(b" \n\t\nvalue\n"));
        assert!(!value.more);
        assert_eq!(value.next_cursor.offset, 10);
    }

    #[test]
    fn batch_capacity_retains_the_next_record_including_blank_byte_cost() {
        let (_directory, session) = fixture(b" \nabc\nxy\n");
        let limits = limits(10, 4, 5);
        let first = FileSessionLoader
            .read_batch(&session, None, limits)
            .unwrap();
        assert_eq!(first.records, vec![]);
        assert_eq!(first.next_cursor.offset, 2);
        assert!(first.more);
        let second = FileSessionLoader
            .read_batch(&session, Some(&first.next_cursor), limits)
            .unwrap();
        assert_eq!(second.records[0].bytes, b"abc");
        assert_eq!(second.records[0].offset, 2);
        assert_eq!(second.next_cursor.offset, 6);
        assert!(second.more);
        let third = FileSessionLoader
            .read_batch(&session, Some(&second.next_cursor), limits)
            .unwrap();
        assert_eq!(third.records[0].bytes, b"xy");
        assert_eq!(third.records[0].offset, 6);
        assert!(!third.more);
    }

    #[test]
    fn lf_and_cr_count_toward_record_limit_and_oversize_retry_loses_nothing() {
        let (_directory, session) = fixture(b"a\nlong\r\n");
        let prior = FileSessionLoader
            .read_batch(&session, None, limits(1, 2, 2))
            .unwrap();
        let failure = FileSessionLoader
            .read_batch(&session, Some(&prior.next_cursor), limits(2, 5, 5))
            .unwrap_err();
        assert_eq!(failure.kind(), PipelineErrorKind::RecordLimit);
        assert_eq!(failure.offset(), Some(2));
        let retry = FileSessionLoader
            .read_batch(&session, Some(&prior.next_cursor), limits(2, 6, 6))
            .unwrap();
        assert_eq!(retry.records[0].bytes, b"long\r");
        assert_eq!(retry.records[0].offset, 2);
        assert_eq!(retry.next_cursor.offset, 8);
        assert!(!retry.more);
    }

    #[test]
    fn oversize_after_prior_records_returns_no_partial_batch_and_retries_all_records() {
        let (_directory, session) = fixture(b"a\n12345\n");
        let failure = FileSessionLoader
            .read_batch(&session, None, limits(10, 5, 20))
            .unwrap_err();
        assert_eq!(failure.kind(), PipelineErrorKind::RecordLimit);
        assert_eq!(failure.offset(), Some(2));
        let retry = FileSessionLoader
            .read_batch(&session, None, limits(10, 6, 20))
            .unwrap();
        assert_eq!(retry.records, fixture_records(b"a\n12345\n"));
        assert_eq!(retry.next_cursor.offset, 8);
    }

    #[test]
    fn oversize_blank_and_unterminated_lines_are_not_silently_skipped() {
        for bytes in [b"     \n".as_slice(), b"123456".as_slice()] {
            let (_directory, session) = fixture(bytes);
            let failure = FileSessionLoader
                .read_batch(&session, None, limits(10, 5, 20))
                .unwrap_err();
            assert_eq!(failure.kind(), PipelineErrorKind::RecordLimit);
            assert_eq!(failure.offset(), Some(0));
        }
    }

    #[test]
    fn split_utf8_tail_waits_for_lf_and_then_emits_from_its_original_offset() {
        let (_directory, session) = fixture(b"a\n\xe2\x82");
        let first = FileSessionLoader
            .read_batch(&session, None, limits(10, 8, 20))
            .unwrap();
        assert_eq!(first.records, fixture_records(b"a\n"));
        assert_eq!(first.next_cursor.offset, 2);
        assert!(first.incomplete_tail);
        assert!(!first.more);
        let unchanged = FileSessionLoader
            .read_batch(&session, Some(&first.next_cursor), limits(10, 2, 20))
            .unwrap();
        assert!(unchanged.incomplete_tail);
        assert_eq!(unchanged.next_cursor.offset, 2);
        assert!(!unchanged.more);
        OpenOptions::new()
            .append(true)
            .open(&session.path)
            .unwrap()
            .write_all(b"\xac\n")
            .unwrap();
        let completed = FileSessionLoader
            .read_batch(&session, Some(&first.next_cursor), limits(10, 4, 20))
            .unwrap();
        assert_eq!(completed.records[0].bytes, "€".as_bytes());
        assert_eq!(completed.records[0].offset, 2);
        assert_eq!(completed.next_cursor.offset, 6);
        assert!(!completed.more);
        assert!(!completed.incomplete_tail);
    }

    #[test]
    fn empty_eof_and_exact_capacity_eof_do_not_request_a_busy_loop() {
        let (_directory, empty) = fixture(b"");
        let batch = FileSessionLoader
            .read_batch(&empty, None, limits(1, 1, 1))
            .unwrap();
        assert_eq!(batch.next_cursor.offset, 0);
        assert!(!batch.more);
        assert!(!batch.incomplete_tail);
        let (_directory, exact) = fixture(b"a\n");
        let first = FileSessionLoader
            .read_batch(&exact, None, limits(1, 2, 2))
            .unwrap();
        assert!(!first.more);
        let end = FileSessionLoader
            .read_batch(&exact, Some(&first.next_cursor), limits(1, 2, 2))
            .unwrap();
        assert_eq!(end.records, vec![]);
        assert_eq!(end.next_cursor.offset, 2);
        assert!(!end.more);
        assert!(!end.incomplete_tail);
    }

    #[test]
    fn cursor_requires_exact_path_identity_and_lf_boundary() {
        let (directory, session) = fixture(b"a\nb\n");
        let first = FileSessionLoader
            .read_batch(&session, None, limits(1, 2, 2))
            .unwrap();
        let mut invalid = first.next_cursor.clone();
        invalid.source = directory.path().join("other.jsonl");
        assert_eq!(
            FileSessionLoader
                .read_batch(&session, Some(&invalid), ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::SourceChanged
        );
        invalid = first.next_cursor.clone();
        invalid.identity = SourceIdentity::Unix {
            device: u64::MAX,
            inode: u64::MAX,
        };
        assert_eq!(
            FileSessionLoader
                .read_batch(&session, Some(&invalid), ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::SourceChanged
        );
        invalid.identity = SourceIdentity::Unavailable;
        assert_eq!(
            FileSessionLoader
                .read_batch(&session, Some(&invalid), ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::Unsupported
        );
        for offset in [1, 3, 5, u64::MAX] {
            invalid = first.next_cursor.clone();
            invalid.offset = offset;
            assert_eq!(
                FileSessionLoader
                    .read_batch(&session, Some(&invalid), ReadLimits::default())
                    .unwrap_err()
                    .kind(),
                PipelineErrorKind::SourceChanged
            );
        }
    }

    #[test]
    fn replacement_and_truncation_reject_old_cursor_instead_of_restarting() {
        let (directory, session) = fixture(b"a\nb\n");
        let first = FileSessionLoader
            .read_batch(&session, None, ReadLimits::default())
            .unwrap();
        OpenOptions::new()
            .write(true)
            .open(&session.path)
            .unwrap()
            .set_len(2)
            .unwrap();
        assert_eq!(
            FileSessionLoader
                .read_batch(&session, Some(&first.next_cursor), ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::SourceChanged
        );
        let replacement = directory.path().join("replacement");
        fs::write(&replacement, b"x\ny\n").unwrap();
        fs::rename(replacement, &session.path).unwrap();
        assert_eq!(
            FileSessionLoader
                .read_batch(&session, Some(&first.next_cursor), ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::SourceChanged
        );
        let fresh = FileSessionLoader
            .read_batch(&session, None, ReadLimits::default())
            .unwrap();
        assert_eq!(fresh.records, fixture_records(b"x\ny\n"));
        fs::remove_file(&session.path).unwrap();
        assert_eq!(
            FileSessionLoader
                .read_batch(&session, Some(&fresh.next_cursor), ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::SourceChanged
        );
    }

    #[test]
    fn rejects_final_symlinks_without_opening_target_or_sidecars() {
        let (directory, session) = fixture(b"native only\n");
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("data"), b"outside\n").unwrap();
        let link = SessionRef {
            path: directory.path().join("link.jsonl"),
        };
        symlink(outside.path().join("data"), &link.path).unwrap();
        assert_eq!(
            FileSessionLoader
                .read_batch(&link, None, ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::Read
        );
        symlink(outside.path(), directory.path().join("sidecar")).unwrap();
        let batch = FileSessionLoader
            .read_batch(&session, None, ReadLimits::default())
            .unwrap();
        assert_eq!(batch.records, fixture_records(b"native only\n"));
        fs::rename(&session.path, directory.path().join("old")).unwrap();
        symlink(outside.path().join("data"), &session.path).unwrap();
        assert_eq!(
            FileSessionLoader
                .read_batch(&session, Some(&batch.next_cursor), ReadLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::SourceChanged
        );
    }

    #[test]
    fn fifo_is_rejected_without_waiting_for_a_writer() {
        let directory = tempdir().unwrap();
        let session = SessionRef {
            path: directory.path().join("pipe.jsonl"),
        };
        let native = CString::new(session.path.to_str().unwrap()).unwrap();
        // SAFETY: native is a live NUL-terminated path; mkfifo retains no pointer.
        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _directory = directory;
            sender
                .send(FileSessionLoader.read_batch(&session, None, ReadLimits::default()))
                .unwrap();
        });
        let failure = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("FIFO open blocked")
            .unwrap_err();
        assert_eq!(failure.kind(), PipelineErrorKind::Read);
    }
}

#[cfg(not(unix))]
#[test]
fn every_filesystem_operation_fails_fast_as_unsupported() {
    // Deliberately invalid input verifies platform refusal precedes validation/I/O.
    assert_eq!(
        FileSessionLoader
            .list_sessions(&SourceScope {
                root: "missing".into(),
                max_sessions: 0
            })
            .unwrap_err()
            .kind(),
        PipelineErrorKind::Unsupported
    );
    assert_eq!(
        FileSessionLoader
            .read_batch(
                &SessionRef {
                    path: "missing".into()
                },
                None,
                ReadLimits {
                    max_records: 0,
                    max_record_bytes: 0,
                    max_batch_bytes: 0
                }
            )
            .unwrap_err()
            .kind(),
        PipelineErrorKind::Unsupported
    );
}
