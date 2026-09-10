//! Explicit Git-object ingestion contracts. No process, filesystem or ambient access.
use crate::{MappingOptions, PipelineError, TelemetryRecord};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, error::Error, fmt, io::Write, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "commits", rename_all = "snake_case")]
pub enum GitNoteSelection {
    All,
    Commits(Vec<String>),
}
impl GitNoteSelection {
    pub fn normalized(&self) -> Self {
        match self {
            Self::All => Self::All,
            Self::Commits(ids) => Self::Commits(
                ids.iter()
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitNotesScope {
    pub repository: PathBuf,
    pub notes_ref: String,
    pub selection: GitNoteSelection,
}
impl GitNotesScope {
    pub fn validate(&self, limits: GitNotesLimits) -> Result<(), GitNotesError> {
        limits.validate()?;
        if !absolute_utf8(&self.repository) || !valid_notes_ref(&self.notes_ref) {
            return Err(GitNotesError::InvalidInput);
        }
        if let GitNoteSelection::Commits(ids) = &self.selection {
            if ids.len() > limits.max_notes {
                return Err(GitNotesError::ListingLimit);
            }
            if ids.iter().any(|id| !valid_object_id(id)) {
                return Err(GitNotesError::InvalidInput);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitNotesLimits {
    pub max_notes: usize,
    pub max_records: usize,
    pub max_note_bytes: usize,
    pub max_total_bytes: usize,
    pub max_listing_bytes: usize,
    pub command_timeout_ms: u64,
}
impl Default for GitNotesLimits {
    fn default() -> Self {
        Self {
            max_notes: 1000,
            max_records: 10_000,
            max_note_bytes: 1_048_576,
            max_total_bytes: 16_777_216,
            max_listing_bytes: 1_048_576,
            command_timeout_ms: 5000,
        }
    }
}
impl GitNotesLimits {
    pub fn validate(self) -> Result<(), GitNotesError> {
        if self.max_notes == 0
            || self.max_notes > 10_000
            || self.max_records == 0
            || self.max_records > 100_000
            || self.max_note_bytes == 0
            || self.max_note_bytes > 33_554_432
            || self.max_total_bytes < self.max_note_bytes
            || self.max_total_bytes > 67_108_864
            || self.max_listing_bytes == 0
            || self.max_listing_bytes > 67_108_864
            || self.command_timeout_ms == 0
            || self.command_timeout_ms > 60_000
        {
            return Err(GitNotesError::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitNoteRef {
    pub repository: PathBuf,
    pub repository_id: PathBuf,
    pub notes_ref: String,
    pub notes_tip: String,
    pub target_commit: String,
    pub note_blob: String,
}
impl GitNoteRef {
    pub fn validate(&self) -> Result<(), GitNotesError> {
        if !absolute_utf8(&self.repository)
            || !absolute_utf8(&self.repository_id)
            || !valid_notes_ref(&self.notes_ref)
            || [&self.notes_tip, &self.target_commit, &self.note_blob]
                .iter()
                .any(|id| !valid_object_id(id) || id.len() != self.notes_tip.len())
        {
            return Err(GitNotesError::InvalidData);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitNotesListing {
    pub repository: PathBuf,
    pub repository_id: PathBuf,
    pub git_dir: PathBuf,
    pub worktree_root: Option<PathBuf>,
    pub notes_ref: String,
    pub notes_tip: Option<String>,
    pub selection: GitNoteSelection,
    pub notes: Vec<GitNoteRef>,
}
impl GitNotesListing {
    pub fn validate(
        &self,
        scope: &GitNotesScope,
        limits: GitNotesLimits,
    ) -> Result<(), GitNotesError> {
        scope.validate(limits)?;
        if !absolute_utf8(&self.repository)
            || !absolute_utf8(&self.repository_id)
            || !absolute_utf8(&self.git_dir)
            || self
                .worktree_root
                .as_ref()
                .is_some_and(|p| !absolute_utf8(p))
            || self.notes_ref != scope.notes_ref
            || self.selection != scope.selection.normalized()
            || self
                .notes_tip
                .as_ref()
                .is_some_and(|id| !valid_object_id(id))
            || (self.notes_tip.is_none() && !self.notes.is_empty())
        {
            return Err(GitNotesError::InvalidData);
        }
        if self.notes.len() > limits.max_notes {
            return Err(GitNotesError::ListingLimit);
        }
        let mut previous: Option<&str> = None;
        for note in &self.notes {
            note.validate()?;
            if note.repository != self.repository
                || note.repository_id != self.repository_id
                || note.notes_ref != self.notes_ref
                || Some(&note.notes_tip) != self.notes_tip.as_ref()
                || previous.is_some_and(|id| id >= note.target_commit.as_str())
                || matches!(&self.selection, GitNoteSelection::Commits(ids) if ids.binary_search(&note.target_commit).is_err())
            {
                return Err(GitNotesError::InvalidData);
            }
            previous = Some(&note.target_commit);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedGitNote {
    pub source: GitNoteRef,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct GitNotesRequest {
    pub scope: GitNotesScope,
    pub limits: GitNotesLimits,
    pub options: MappingOptions,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitNotesCollection {
    pub listing: GitNotesListing,
    /// Includes the closing selection manifest.
    pub records_written: usize,
}

pub trait GitNoteLoader: Send + Sync {
    fn list_notes(
        &self,
        scope: &GitNotesScope,
        limits: GitNotesLimits,
    ) -> Result<GitNotesListing, GitNotesError>;
    fn read_note(
        &self,
        source: &GitNoteRef,
        limits: GitNotesLimits,
    ) -> Result<LoadedGitNote, GitNotesError>;
}
pub trait GitNoteAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn map_note(
        &self,
        note: &LoadedGitNote,
        options: MappingOptions,
        limits: GitNotesLimits,
    ) -> Result<Vec<TelemetryRecord>, GitNotesError>;
}
pub trait GitNotesApi: Send + Sync {
    fn list_notes(
        &self,
        scope: &GitNotesScope,
        limits: GitNotesLimits,
    ) -> Result<GitNotesListing, GitNotesError>;
    fn collect_notes(
        &self,
        request: &GitNotesRequest,
        destination: &mut dyn Write,
    ) -> Result<GitNotesCollection, GitNotesError>;
}

/// Safe, typed failures; raw process stderr and note payloads never cross this boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitNotesError {
    InvalidInput,
    InvalidRef,
    GitUnavailable,
    UnsafeRepository,
    ObjectRead,
    Timeout,
    UnsupportedPlatform,
    UnsupportedRepository,
    UnsupportedTarget,
    UnsupportedFormat,
    NoteLimit,
    ListingLimit,
    BatchLimit,
    RecordLimit,
    InvalidData,
    Output(PipelineError),
}
impl GitNotesError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::InvalidRef => "invalid_ref",
            Self::GitUnavailable => "git_unavailable",
            Self::UnsafeRepository => "unsafe_repository",
            Self::ObjectRead => "object_read",
            Self::Timeout => "timeout",
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::UnsupportedRepository => "unsupported_repository",
            Self::UnsupportedTarget => "unsupported_target",
            Self::UnsupportedFormat => "unsupported_format",
            Self::NoteLimit => "note_limit",
            Self::ListingLimit => "listing_limit",
            Self::BatchLimit => "batch_limit",
            Self::RecordLimit => "record_limit",
            Self::InvalidData => "invalid_data",
            Self::Output(_) => "output",
        }
    }
    pub fn message(&self) -> &'static str {
        match self {
            Self::InvalidInput => {
                "Use an absolute UTF-8 repository, a full refs/notes/* ref, full lowercase commit IDs and positive bounded limits."
            }
            Self::InvalidRef => "The selected notes ref is invalid or does not point to a commit.",
            Self::GitUnavailable => {
                "Standard Git could not execute. Supply --git-executable with an absolute executable path; Git AI is not required."
            }
            Self::UnsafeRepository => {
                "Git refused repository ownership. Use a caller-owned checkout or have the operator establish trust separately; Unisphere does not override safe.directory."
            }
            Self::ObjectRead => {
                "The selected local repository or pinned Git object could not be read; no fetch was attempted."
            }
            Self::Timeout => {
                "A Git command exceeded the selected deadline; no collection was accepted."
            }
            Self::UnsupportedPlatform => {
                "The Git-object loader currently supports Unix; supplied-data mapping is platform-independent."
            }
            Self::UnsupportedRepository => {
                "Partial-clone or promisor repositories are refused to prevent implicit fetching. Use a complete local repository."
            }
            Self::UnsupportedTarget => {
                "A selected note targets a non-commit object; select commit-attached notes explicitly."
            }
            Self::UnsupportedFormat => {
                "The note schema or record variant is unsupported; no note was silently skipped."
            }
            Self::NoteLimit => {
                "A note exceeds max-note-bytes; increase the compatible note/total budgets or select different commits."
            }
            Self::ListingLimit => {
                "The selection or Git listing exceeds its budget; select specific commits or increase max-notes/max-listing-bytes."
            }
            Self::BatchLimit => {
                "The selected note bytes exceed max-total-bytes; select fewer commits or increase the budget."
            }
            Self::RecordLimit => {
                "The mapped record count exceeds max-records; select fewer commits or increase the budget."
            }
            Self::InvalidData => {
                "The note data or supplied Git-object provenance is malformed; no collection was accepted."
            }
            Self::Output(_) => {
                "Output failed; discard any partial destination bytes before retrying."
            }
        }
    }
}
impl fmt::Display for GitNotesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind(), self.message())?;
        if let Self::Output(error) = self {
            write!(f, " {error}")?;
        }
        Ok(())
    }
}
impl Error for GitNotesError {}
impl From<PipelineError> for GitNotesError {
    fn from(error: PipelineError) -> Self {
        Self::Output(error)
    }
}

pub fn valid_object_id(id: &str) -> bool {
    matches!(id.len(), 40 | 64)
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn valid_notes_ref(name: &str) -> bool {
    name.starts_with("refs/notes/")
        && !name.contains("..")
        && !name.contains("@{")
        && !name
            .bytes()
            .any(|b| b <= b' ' || b == 127 || b"~^:?*[\\".contains(&b))
        && name.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && !part.ends_with('.')
                && !part.ends_with(".lock")
        })
}
fn absolute_utf8(path: &std::path::Path) -> bool {
    path.is_absolute() && path.to_str().is_some_and(|s| !s.contains('\0'))
}

/// Bound repeated string payloads before staging amplified attribution records.
/// This is only a lower bound on OTLP bytes; the writer still enforces encoded size.
pub fn account_git_record(
    record: &TelemetryRecord,
    total: &mut usize,
) -> Result<(), GitNotesError> {
    fn strings(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::String(value) => value.len(),
            serde_json::Value::Array(values) => values
                .iter()
                .fold(0usize, |sum, value| sum.saturating_add(strings(value))),
            serde_json::Value::Object(values) => values.iter().fold(0usize, |sum, (key, value)| {
                sum.saturating_add(key.len()).saturating_add(strings(value))
            }),
            _ => 0,
        }
    }
    *total = record.attributes.iter().fold(
        total.saturating_add(record.event_name.len()),
        |sum, (key, value)| sum.saturating_add(key.len()).saturating_add(strings(value)),
    );
    if let Some(body) = &record.body {
        *total = total.saturating_add(strings(body));
    }
    if *total > crate::MAX_OUTPUT_BATCH_BYTES {
        return Err(GitNotesError::Output(PipelineError::new(
            crate::PipelineErrorKind::OutputLimit,
            None,
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_selectors_and_limits_fail_before_ports_are_needed() {
        let mut scope = GitNotesScope {
            repository: "/repo".into(),
            notes_ref: "refs/notes/ai".into(),
            selection: GitNoteSelection::Commits(vec![]),
        };
        assert!(scope.validate(GitNotesLimits::default()).is_ok());
        for name in [
            "--help",
            "refs/notes/../ai",
            "refs/notes/a.lock",
            "refs/notes/a\n",
            "refs/notes/",
        ] {
            scope.notes_ref = name.into();
            assert_eq!(
                scope.validate(GitNotesLimits::default()),
                Err(GitNotesError::InvalidInput)
            );
        }
        let limits = GitNotesLimits {
            max_records: 0,
            ..GitNotesLimits::default()
        };
        assert_eq!(limits.validate(), Err(GitNotesError::InvalidInput));
        assert!(!valid_object_id(&"a".repeat(41)));
    }
}
