//! Whole selected-ref projections over injected Git storage, mapping and output ports.
use serde_json::json;
use std::{collections::BTreeMap, io::Write};
use unisphere_core::{
    GitNoteAdapter, GitNoteLoader, GitNoteSelection, GitNotesApi, GitNotesCollection,
    GitNotesError, GitNotesLimits, GitNotesListing, GitNotesRequest, GitNotesScope, RecordWriter,
    TelemetryRecord, account_git_record,
};

pub struct GitNotesCollector<L, A, W> {
    loader: L,
    adapter: A,
    writer: W,
}
impl<L, A, W> GitNotesCollector<L, A, W> {
    pub fn new(loader: L, adapter: A, writer: W) -> Self {
        Self {
            loader,
            adapter,
            writer,
        }
    }
}
impl<L: GitNoteLoader, A: GitNoteAdapter, W: RecordWriter> GitNotesApi
    for GitNotesCollector<L, A, W>
{
    fn list_notes(
        &self,
        scope: &GitNotesScope,
        limits: GitNotesLimits,
    ) -> Result<GitNotesListing, GitNotesError> {
        scope.validate(limits)?;
        let listing = self.loader.list_notes(scope, limits)?;
        listing.validate(scope, limits)?;
        Ok(listing)
    }
    fn collect_notes(
        &self,
        request: &GitNotesRequest,
        destination: &mut dyn Write,
    ) -> Result<GitNotesCollection, GitNotesError> {
        let listing = self.list_notes(&request.scope, request.limits)?;
        let mut records = Vec::new();
        let mut native_bytes = 0usize;
        let mut payload_bytes = 0usize;
        for source in &listing.notes {
            if records.len() >= request.limits.max_records - 1 {
                return Err(GitNotesError::RecordLimit);
            }
            let note = self.loader.read_note(source, request.limits)?;
            if note.source != *source {
                return Err(GitNotesError::InvalidData);
            }
            if note.bytes.len() > request.limits.max_note_bytes {
                return Err(GitNotesError::NoteLimit);
            }
            native_bytes = native_bytes
                .checked_add(note.bytes.len())
                .ok_or(GitNotesError::BatchLimit)?;
            if native_bytes > request.limits.max_total_bytes {
                return Err(GitNotesError::BatchLimit);
            }
            let limits = GitNotesLimits {
                max_records: request.limits.max_records - records.len() - 1,
                ..request.limits
            };
            let mapped = self.adapter.map_note(&note, request.options, limits)?;
            if mapped.len() > limits.max_records {
                return Err(GitNotesError::RecordLimit);
            }
            for record in &mapped {
                account_git_record(record, &mut payload_bytes)?;
            }
            records.extend(mapped);
        }
        let mut selection = json!({"repository":listing.repository,"notes_ref":listing.notes_ref});
        match &listing.selection {
            GitNoteSelection::All => selection["mode"] = json!("all"),
            GitNoteSelection::Commits(ids) => {
                selection["mode"] = json!("commits");
                selection["commits"] = json!(ids);
            }
        }
        let manifest = TelemetryRecord {
            event_name: "unisphere.git_notes.snapshot".into(),
            timestamp_unix_nano: None,
            body: None,
            attributes: BTreeMap::from([
                ("unisphere.profile.version".into(), json!(1)),
                (
                    "unisphere.source.adapter".into(),
                    json!(self.adapter.name()),
                ),
                ("unisphere.source.path".into(), json!(listing.repository)),
                ("unisphere.source.kind".into(), json!("notes_manifest")),
                ("unisphere.source.key".into(), json!("$git-notes")),
                ("unisphere.source.format".into(), json!("git_notes")),
                ("unisphere.source.revision".into(), json!(listing.notes_tip)),
                (
                    "unisphere.git.repository.id".into(),
                    json!(listing.repository_id),
                ),
                ("unisphere.git.notes.ref".into(), json!(listing.notes_ref)),
                ("unisphere.git.notes.tip".into(), json!(listing.notes_tip)),
                (
                    "unisphere.git_notes.semantics".into(),
                    json!("replace_projection"),
                ),
                ("unisphere.git_notes.records".into(), json!(records.len())),
                (
                    "unisphere.git_notes.notes".into(),
                    json!(listing.notes.len()),
                ),
                ("unisphere.git_notes.selection".into(), selection),
                (
                    "unisphere.git_notes.include_content".into(),
                    json!(request.options.include_content),
                ),
                ("unisphere.git_notes.finality".into(), json!("unknown")),
                (
                    "unisphere.git_notes.ref_state".into(),
                    json!(if listing.notes_tip.is_some() {
                        "present"
                    } else {
                        "missing"
                    }),
                ),
            ]),
        };
        account_git_record(&manifest, &mut payload_bytes)?;
        records.push(manifest);
        self.writer.write_batch(&records, destination)?;
        Ok(GitNotesCollection {
            listing,
            records_written: records.len(),
        })
    }
}
