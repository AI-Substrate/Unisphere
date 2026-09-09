//! In-memory collection fakes and shared pure-adapter conformance assertions.
use std::{io::Write, sync::Mutex};

use unisphere_core::{
    CollectionApi, CollectionBatch, LoadedBatch, MappedBatch, MappingOptions, NativeRecord,
    PipelineError, PipelineErrorKind, ReadCursor, ReadLimits, RecordWriter, SessionAdapter,
    SessionLoader, SessionRef, SourceScope, TelemetryRecord,
};

pub const CLAUDE_BASIC: &[u8] = include_bytes!("../fixtures/collection/claude-basic.jsonl");
pub const CLAUDE_PARTS: &[u8] = include_bytes!("../fixtures/collection/claude-parts.jsonl");

/// Frame known complete fixture bytes without doing storage I/O.
/// Blank means every physical byte is ASCII whitespace (including space, tab,
/// CR and LF); blanks consume offsets and the loader's physical record budget.
pub fn fixture_records(bytes: &[u8]) -> Vec<NativeRecord> {
    let mut offset = 0_u64;
    let mut result = Vec::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        assert_eq!(
            line.last(),
            Some(&b'\n'),
            "fixture must end every record with LF"
        );
        if !line.iter().all(u8::is_ascii_whitespace) {
            result.push(NativeRecord {
                offset,
                bytes: line[..line.len() - 1].to_vec(),
            });
        }
        offset += line.len() as u64;
    }
    result
}

/// Functional second adapter for fixtures and the authoring example: each supplied
/// UTF-8 record is one text fragment, independent of Claude and all storage.
pub struct TextFixtureAdapter;

impl SessionAdapter for TextFixtureAdapter {
    fn name(&self) -> &'static str {
        "fixture-text"
    }
    fn map(
        &self,
        source: &SessionRef,
        input: &[NativeRecord],
        options: MappingOptions,
    ) -> Result<MappedBatch, PipelineError> {
        source.validate()?;
        let mut mapped = MappedBatch::default();
        for native in input {
            let text = std::str::from_utf8(&native.bytes).map_err(|_| {
                PipelineError::new(PipelineErrorKind::InvalidData, Some(native.offset))
            })?;
            let mut attributes = std::collections::BTreeMap::from([
                ("unisphere.profile.version".to_owned(), serde_json::json!(1)),
                (
                    "unisphere.source.adapter".to_owned(),
                    serde_json::json!(self.name()),
                ),
                (
                    "unisphere.source.path".to_owned(),
                    serde_json::json!(source.path.to_str().unwrap()),
                ),
                (
                    "unisphere.source.offset".to_owned(),
                    serde_json::json!(native.offset),
                ),
                (
                    "unisphere.source.kind".to_owned(),
                    serde_json::json!("text"),
                ),
            ]);
            let body = if options.include_content {
                Some(serde_json::json!({"role":"user","parts":[{"type":"text","content":text}]}))
            } else {
                attributes.insert("unisphere.content.omitted".into(), serde_json::json!(true));
                mapped.diagnostics.push(unisphere_core::MappingDiagnostic {
                    offset: native.offset,
                    code: unisphere_core::MappingDiagnosticCode::ContentOmitted,
                });
                None
            };
            mapped.records.push(TelemetryRecord {
                event_name: "unisphere.session.record".into(),
                timestamp_unix_nano: None,
                attributes,
                body,
            });
        }
        Ok(mapped)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCall {
    pub session: SessionRef,
    pub cursor: Option<ReadCursor>,
    pub limits: ReadLimits,
}

pub struct FakeSessionLoader {
    sessions: Result<Vec<SessionRef>, PipelineError>,
    batch: Result<LoadedBatch, PipelineError>,
    list_calls: Mutex<Vec<SourceScope>>,
    read_calls: Mutex<Vec<BatchCall>>,
}

impl FakeSessionLoader {
    pub fn new(
        sessions: Result<Vec<SessionRef>, PipelineError>,
        batch: Result<LoadedBatch, PipelineError>,
    ) -> Self {
        Self {
            sessions,
            batch,
            list_calls: Mutex::new(Vec::new()),
            read_calls: Mutex::new(Vec::new()),
        }
    }
    pub fn list_calls(&self) -> Vec<SourceScope> {
        self.list_calls.lock().unwrap().clone()
    }
    pub fn read_calls(&self) -> Vec<BatchCall> {
        self.read_calls.lock().unwrap().clone()
    }
}
impl SessionLoader for FakeSessionLoader {
    fn list_sessions(&self, scope: &SourceScope) -> Result<Vec<SessionRef>, PipelineError> {
        self.list_calls.lock().unwrap().push(scope.clone());
        self.sessions.clone()
    }
    fn read_batch(
        &self,
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
    ) -> Result<LoadedBatch, PipelineError> {
        self.read_calls.lock().unwrap().push(BatchCall {
            session: session.clone(),
            cursor: cursor.cloned(),
            limits,
        });
        self.batch.clone()
    }
}

pub struct FakeRecordWriter {
    result: Result<(), PipelineError>,
    output: Vec<u8>,
    calls: Mutex<Vec<Vec<TelemetryRecord>>>,
}
impl FakeRecordWriter {
    pub fn new(result: Result<(), PipelineError>) -> Self {
        Self {
            result,
            output: Vec::new(),
            calls: Mutex::new(Vec::new()),
        }
    }
    pub fn with_output(mut self, output: Vec<u8>) -> Self {
        self.output = output;
        self
    }
    pub fn calls(&self) -> Vec<Vec<TelemetryRecord>> {
        self.calls.lock().unwrap().clone()
    }
}
impl RecordWriter for FakeRecordWriter {
    fn write_batch(
        &self,
        records: &[TelemetryRecord],
        destination: &mut dyn Write,
    ) -> Result<(), PipelineError> {
        self.calls.lock().unwrap().push(records.to_vec());
        self.result.clone()?;
        destination
            .write_all(&self.output)
            .and_then(|()| destination.flush())
            .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionCall {
    pub batch: BatchCall,
    pub options: MappingOptions,
}

pub struct FakeCollector {
    sessions: Result<Vec<SessionRef>, PipelineError>,
    batch: Result<CollectionBatch, PipelineError>,
    output: Vec<u8>,
    list_calls: Mutex<Vec<SourceScope>>,
    calls: Mutex<Vec<CollectionCall>>,
}
impl FakeCollector {
    pub fn new(
        sessions: Result<Vec<SessionRef>, PipelineError>,
        batch: Result<CollectionBatch, PipelineError>,
    ) -> Self {
        Self {
            sessions,
            batch,
            output: Vec::new(),
            list_calls: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
        }
    }
    pub fn with_output(mut self, output: Vec<u8>) -> Self {
        self.output = output;
        self
    }
    pub fn list_calls(&self) -> Vec<SourceScope> {
        self.list_calls.lock().unwrap().clone()
    }
    pub fn calls(&self) -> Vec<CollectionCall> {
        self.calls.lock().unwrap().clone()
    }
}
impl CollectionApi for FakeCollector {
    fn list_sessions(&self, scope: &SourceScope) -> Result<Vec<SessionRef>, PipelineError> {
        self.list_calls.lock().unwrap().push(scope.clone());
        self.sessions.clone()
    }
    fn collect_batch(
        &self,
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
        options: MappingOptions,
        destination: &mut dyn Write,
    ) -> Result<CollectionBatch, PipelineError> {
        self.calls.lock().unwrap().push(CollectionCall {
            batch: BatchCall {
                session: session.clone(),
                cursor: cursor.cloned(),
                limits,
            },
            options,
        });
        let batch = self.batch.clone()?;
        destination
            .write_all(&self.output)
            .and_then(|()| destination.flush())
            .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
        Ok(batch)
    }
}

/// Applies to valid, complete records supported by a fixture adapter.
/// It checks common semantics, not Claude-specific key names or invented totals.
/// Output bounds here mean one mapped record per supplied physical record.
/// The OTLP writer separately enforces MAX_OUTPUT_BATCH_BYTES while encoding:
/// serializing these Rust DTOs would not measure the OTLP representation.
/// Fixture content uses reserved SENSITIVE-* markers, never metadata fields.
pub fn assert_adapter_conformance(
    adapter: &dyn SessionAdapter,
    source: &SessionRef,
    records: &[NativeRecord],
) {
    let metadata = adapter
        .map(source, records, MappingOptions::default())
        .expect("valid fixture mapping");
    let repeated = adapter
        .map(source, records, MappingOptions::default())
        .expect("deterministic fixture mapping");
    assert_eq!(metadata, repeated, "equal input must produce equal mapping");
    assert_eq!(
        metadata.records.len(),
        records.len(),
        "retain every physical fixture record"
    );
    let metadata_json = serde_json::to_string(&metadata).expect("serializable metadata");
    assert!(
        !metadata_json.contains("SENSITIVE-"),
        "fixture content escaped metadata-only policy"
    );
    for (record, native) in metadata.records.iter().zip(records) {
        assert_eq!(record.event_name, "unisphere.session.record");
        assert_eq!(
            record.attributes["unisphere.profile.version"],
            serde_json::json!(1)
        );
        assert_eq!(
            record.attributes["unisphere.source.adapter"],
            adapter.name()
        );
        assert_eq!(
            record.attributes["unisphere.source.path"],
            source.path.to_str().unwrap()
        );
        assert_eq!(
            record.attributes["unisphere.source.offset"],
            serde_json::json!(native.offset)
        );
        assert!(record.attributes["unisphere.source.kind"].is_string());
        assert!(
            record.body.is_none(),
            "metadata-only mapping must not emit content bodies"
        );
    }
    let content = adapter
        .map(
            source,
            records,
            MappingOptions {
                include_content: true,
            },
        )
        .expect("opt-in fixture mapping");
    assert_eq!(content.records.len(), records.len());
    for (record, metadata) in content.records.iter().zip(&metadata.records) {
        for key in [
            "unisphere.source.path",
            "unisphere.source.offset",
            "unisphere.source.adapter",
            "unisphere.source.kind",
        ] {
            assert_eq!(
                record.attributes[key], metadata.attributes[key],
                "content policy cannot change provenance"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unisphere_core::SourceIdentity;

    #[test]
    fn another_pure_adapter_conforms_without_storage_or_claude() {
        let source = SessionRef {
            path: "/fixture/text.jsonl".into(),
        };
        let records = fixture_records(b"SENSITIVE-TEXT\nsecond text\n");
        assert_adapter_conformance(&TextFixtureAdapter, &source, &records);
        let content = TextFixtureAdapter
            .map(
                &source,
                &records,
                MappingOptions {
                    include_content: true,
                },
            )
            .unwrap();
        assert_eq!(
            content.records[0].body.as_ref().unwrap()["parts"][0]["content"],
            "SENSITIVE-TEXT"
        );
        let invalid = [NativeRecord {
            offset: 8,
            bytes: vec![0xff],
        }];
        assert_eq!(
            TextFixtureAdapter
                .map(&source, &invalid, MappingOptions::default())
                .unwrap_err()
                .offset(),
            Some(8)
        );
    }

    fn batch() -> CollectionBatch {
        CollectionBatch {
            mapped: MappedBatch::default(),
            next_cursor: ReadCursor {
                source: "/fixture/session.jsonl".into(),
                identity: SourceIdentity::Unix {
                    device: 1,
                    inode: 2,
                },
                offset: 3,
            },
            more: false,
            incomplete_tail: false,
        }
    }

    #[test]
    fn fixture_framing_preserves_physical_offsets_across_blanks_and_crlf() {
        let records = fixture_records(b" \n{}\r\n[]\n");
        assert_eq!(
            records,
            vec![
                NativeRecord {
                    offset: 2,
                    bytes: b"{}\r".to_vec()
                },
                NativeRecord {
                    offset: 6,
                    bytes: b"[]".to_vec()
                }
            ]
        );
    }

    #[test]
    fn fake_collector_returns_checkpoint_only_when_output_is_accepted() {
        struct Reject;
        impl Write for Reject {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let expected = batch();
        let fake = FakeCollector::new(Ok(Vec::new()), Ok(expected.clone()))
            .with_output(b"record\n".to_vec());
        let source = SessionRef {
            path: expected.next_cursor.source.clone(),
        };
        let collector: &dyn CollectionApi = &fake;
        assert_eq!(
            collector
                .collect_batch(
                    &source,
                    None,
                    ReadLimits::default(),
                    MappingOptions::default(),
                    &mut Reject
                )
                .unwrap_err()
                .kind(),
            PipelineErrorKind::Write
        );
        let mut output = Vec::new();
        assert_eq!(
            collector
                .collect_batch(
                    &source,
                    None,
                    ReadLimits::default(),
                    MappingOptions::default(),
                    &mut output
                )
                .unwrap(),
            expected
        );
        assert_eq!(output, b"record\n");
    }

    #[test]
    fn configured_loader_error_does_not_touch_any_real_source() {
        let failure = PipelineError::new(PipelineErrorKind::SourceChanged, Some(42));
        let fake = FakeSessionLoader::new(Err(failure.clone()), Err(failure.clone()));
        let source = SessionRef {
            path: "/does/not/exist/session.jsonl".into(),
        };
        assert_eq!(
            fake.read_batch(&source, None, ReadLimits::default())
                .unwrap_err(),
            failure
        );
        assert_eq!(fake.read_calls()[0].session, source);
    }
}
