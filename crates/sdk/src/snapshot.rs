//! Revision-snapshot collection through injected storage, mapping and output.
use serde_json::json;
use std::{collections::BTreeMap, io::Write};
use unisphere_core::{
    PipelineError, PipelineErrorKind, RecordWriter, SnapshotAdapter, SnapshotCheckpoint,
    SnapshotCollection, SnapshotCollectionApi, SnapshotFormat, SnapshotLoader, SnapshotRequest,
    TelemetryRecord,
};

pub struct SnapshotCollector<L, A, W> {
    loader: L,
    adapter: A,
    writer: W,
}
impl<L, A, W> SnapshotCollector<L, A, W> {
    pub fn new(loader: L, adapter: A, writer: W) -> Self {
        Self {
            loader,
            adapter,
            writer,
        }
    }
}
impl<L: SnapshotLoader, A: SnapshotAdapter, W: RecordWriter> SnapshotCollectionApi
    for SnapshotCollector<L, A, W>
{
    fn collect_snapshot(
        &self,
        request: &SnapshotRequest,
        destination: &mut dyn Write,
    ) -> Result<SnapshotCollection, PipelineError> {
        request.source.validate()?;
        request.limits.validate()?;
        let snapshot = self.loader.read_snapshot(&request.source, request.limits)?;
        snapshot.validate(request.limits)?;
        if snapshot.source != request.source {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        }
        let mut mapped = self.adapter.map_snapshot(&snapshot, request.options)?;
        let format = match snapshot.source.format {
            SnapshotFormat::JsonDocument => "json_document",
            SnapshotFormat::JsonJournal => "json_journal",
            SnapshotFormat::SqliteKeyValue { .. } => "sqlite_key_value",
        };
        let path = snapshot
            .source
            .path
            .to_str()
            .ok_or_else(|| PipelineError::new(PipelineErrorKind::InvalidData, None))?;
        let mut attributes = BTreeMap::from([
            (
                "unisphere.source.adapter".into(),
                json!(self.adapter.name()),
            ),
            ("unisphere.source.path".into(), json!(path)),
            ("unisphere.source.key".into(), json!("$snapshot")),
            ("unisphere.source.revision".into(), json!(snapshot.revision)),
            ("unisphere.source.format".into(), json!(format)),
            ("unisphere.source.kind".into(), json!("snapshot_manifest")),
            ("unisphere.profile.version".into(), json!(1)),
            (
                "unisphere.snapshot.semantics".into(),
                json!("replace_projection"),
            ),
            (
                "unisphere.snapshot.records".into(),
                json!(mapped.records.len()),
            ),
            ("unisphere.snapshot.finality".into(), json!("unknown")),
            (
                "unisphere.snapshot.selection".into(),
                json!(snapshot.source),
            ),
            (
                "unisphere.snapshot.include_content".into(),
                json!(request.options.include_content),
            ),
        ]);
        if let Some(session_id) = &snapshot.source.session_id {
            attributes.insert(
                "unisphere.snapshot.requested_session.id".into(),
                json!(session_id),
            );
        }
        // A closing manifest also represents an empty projection. Encoding remains
        // one writer batch, so its size/encoding checks happen before any output.
        mapped.records.push(TelemetryRecord {
            event_name: "unisphere.session.snapshot".into(),
            timestamp_unix_nano: None,
            attributes,
            body: None,
        });
        self.writer.write_batch(&mapped.records, destination)?;
        Ok(SnapshotCollection {
            checkpoint: SnapshotCheckpoint {
                source: snapshot.source,
                revision: snapshot.revision,
                adapter: self.adapter.name().into(),
                include_content: request.options.include_content,
            },
            records_written: mapped.records.len(),
            diagnostics: mapped.diagnostics,
        })
    }
}
