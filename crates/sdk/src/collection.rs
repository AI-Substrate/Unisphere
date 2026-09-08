//! Collection orchestration over injected storage, pure mapping and output ports.

use std::io::Write;
use unisphere_core::{
    CollectionApi, CollectionBatch, MappingOptions, PipelineError, PipelineErrorKind, ReadCursor,
    ReadLimits, RecordWriter, SessionAdapter, SessionLoader, SessionRef, SourceScope,
};

pub struct Collector<L, A, W> {
    loader: L,
    adapter: A,
    writer: W,
}

impl<L, A, W> Collector<L, A, W> {
    pub fn new(loader: L, adapter: A, writer: W) -> Self {
        Self {
            loader,
            adapter,
            writer,
        }
    }
}

impl<L: SessionLoader, A: SessionAdapter, W: RecordWriter> CollectionApi for Collector<L, A, W> {
    fn list_sessions(&self, scope: &SourceScope) -> Result<Vec<SessionRef>, PipelineError> {
        scope.validate()?;
        let sessions = self.loader.list_sessions(scope)?;
        if sessions.len() > scope.max_sessions {
            return Err(PipelineError::new(PipelineErrorKind::ListingLimit, None));
        }
        for session in &sessions {
            session.validate()?;
        }
        Ok(sessions)
    }

    fn collect_batch(
        &self,
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
        options: MappingOptions,
        destination: &mut dyn Write,
    ) -> Result<CollectionBatch, PipelineError> {
        collect_batch(
            &self.loader,
            &self.adapter,
            &self.writer,
            session,
            cursor,
            limits,
            options,
            destination,
        )
    }
}

/// Collect one bounded batch; no checkpoint is returned on any failure.
///
/// This explicit-port entry point is also used by the object-safe [`CollectionApi`]
/// implementation. A failed destination may contain partial bytes; callers choose
/// how to recover those bytes before retrying their previous checkpoint.
#[expect(
    clippy::too_many_arguments,
    reason = "The reviewed public contract keeps all three injected ports and caller-owned batch inputs explicit"
)]
pub fn collect_batch(
    loader: &dyn SessionLoader,
    adapter: &dyn SessionAdapter,
    writer: &dyn RecordWriter,
    session: &SessionRef,
    cursor: Option<&ReadCursor>,
    limits: ReadLimits,
    options: MappingOptions,
    destination: &mut dyn Write,
) -> Result<CollectionBatch, PipelineError> {
    limits.validate()?;
    session.validate()?;
    if cursor.is_some_and(|cursor| cursor.source != session.path) {
        return Err(PipelineError::new(
            PipelineErrorKind::SourceChanged,
            cursor.map(|c| c.offset),
        ));
    }
    let loaded = loader.read_batch(session, cursor, limits)?;
    loaded.validate(limits)?;
    let start = cursor.map_or(0, |cursor| cursor.offset);
    if loaded.source != *session
        || loaded.next_cursor.source != session.path
        || loaded.next_cursor.offset < start
        || (loaded.more && loaded.next_cursor.offset == start)
        || loaded.records.iter().any(|record| record.offset < start)
        || cursor.is_some_and(|cursor| loaded.next_cursor.identity != cursor.identity)
    {
        return Err(PipelineError::new(
            PipelineErrorKind::InvalidData,
            Some(start),
        ));
    }
    let mapped = adapter.map(session, &loaded.records, options)?;
    writer.write_batch(&mapped.records, destination)?;
    Ok(CollectionBatch {
        mapped,
        next_cursor: loaded.next_cursor,
        more: loaded.more,
        incomplete_tail: loaded.incomplete_tail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use unisphere_core::{LoadedBatch, NativeRecord, SourceIdentity};
    use unisphere_testkit::collection::{FakeRecordWriter, FakeSessionLoader, TextFixtureAdapter};

    fn source() -> SessionRef {
        SessionRef {
            path: "/fixture/session.jsonl".into(),
        }
    }
    fn loaded() -> LoadedBatch {
        LoadedBatch {
            source: source(),
            records: vec![NativeRecord {
                offset: 0,
                bytes: b"hello".to_vec(),
            }],
            next_cursor: ReadCursor {
                source: source().path,
                identity: SourceIdentity::Unix {
                    device: 1,
                    inode: 2,
                },
                offset: 6,
            },
            more: false,
            incomplete_tail: false,
        }
    }

    #[test]
    fn invalid_limits_do_not_reach_the_loader() {
        let loader = FakeSessionLoader::new(Ok(vec![source()]), Ok(loaded()));
        let writer = FakeRecordWriter::new(Ok(()));
        let error = collect_batch(
            &loader,
            &TextFixtureAdapter,
            &writer,
            &source(),
            None,
            ReadLimits {
                max_records: 0,
                ..ReadLimits::default()
            },
            MappingOptions::default(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
        assert!(loader.read_calls().is_empty());
        assert!(writer.calls().is_empty());
    }

    #[test]
    fn incomplete_tail_is_returned_after_complete_records_only() {
        let mut batch = loaded();
        batch.incomplete_tail = true;
        let loader = FakeSessionLoader::new(Ok(vec![]), Ok(batch.clone()));
        let writer = FakeRecordWriter::new(Ok(()));
        let result = collect_batch(
            &loader,
            &TextFixtureAdapter,
            &writer,
            &source(),
            None,
            ReadLimits::default(),
            MappingOptions {
                include_content: true,
            },
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(result.next_cursor, batch.next_cursor);
        assert!(result.incomplete_tail);
        assert!(!result.more);
        assert_eq!(
            result.mapped.records[0].body.as_ref().unwrap()["parts"][0]["content"],
            "hello"
        );
    }

    #[test]
    fn write_failure_returns_no_checkpoint_and_leaves_input_cursor_unchanged() {
        let loader = FakeSessionLoader::new(Ok(vec![]), Ok(loaded()));
        let writer = FakeRecordWriter::new(Err(PipelineError::new(PipelineErrorKind::Write, None)));
        let cursor = ReadCursor {
            offset: 0,
            ..loaded().next_cursor
        };
        let before = cursor.clone();
        let error = collect_batch(
            &loader,
            &TextFixtureAdapter,
            &writer,
            &source(),
            Some(&cursor),
            ReadLimits::default(),
            MappingOptions::default(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::Write);
        assert_eq!(cursor, before);
    }

    #[test]
    fn an_injected_loader_cannot_force_a_no_progress_loop() {
        let mut batch = loaded();
        batch.records.clear();
        batch.next_cursor.offset = 0;
        batch.more = true;
        let loader = FakeSessionLoader::new(Ok(vec![]), Ok(batch));
        let writer = FakeRecordWriter::new(Ok(()));
        let error = collect_batch(
            &loader,
            &TextFixtureAdapter,
            &writer,
            &source(),
            None,
            ReadLimits::default(),
            MappingOptions::default(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert!(writer.calls().is_empty());
    }
}
