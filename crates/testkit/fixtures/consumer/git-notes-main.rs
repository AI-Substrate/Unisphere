use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};
use unisphere_adapter_git_ai::GitAiAdapter;
use unisphere_loader_git::GitObjectLoader;
use unisphere_output_otlp::OtlpJsonlWriter;
use unisphere_sdk::{
    GitNoteLoader, GitNoteRef, GitNoteSelection, GitNotesApi, GitNotesCollector, GitNotesError,
    GitNotesLimits, GitNotesRequest, GitNotesScope, MappingOptions,
};

struct BrokenOutput {
    accepted: usize,
    fail_flush: bool,
}
impl Write for BrokenOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fail_flush {
            self.accepted += bytes.len();
            Ok(bytes.len())
        } else if self.accepted == 0 && !bytes.is_empty() {
            self.accepted = 1;
            Ok(1)
        } else {
            Err(io::Error::other("synthetic destination failure"))
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("synthetic flush failure"))
    }
}
fn run() -> Result<(), GitNotesError> {
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).ok_or(GitNotesError::InvalidInput)?;
    let repository = args.get(2).ok_or(GitNotesError::InvalidInput)?.into();
    let git = args.get(3).ok_or(GitNotesError::InvalidInput)?.into();
    let loader = GitObjectLoader::new(git);
    if mode == "read" {
        let source: GitNoteRef =
            serde_json::from_str(args.get(4).ok_or(GitNotesError::InvalidInput)?)
                .map_err(|_| GitNotesError::InvalidInput)?;
        let note = loader.read_note(&source, GitNotesLimits::default())?;
        io::stdout()
            .lock()
            .write_all(&note.bytes)
            .map_err(|_| GitNotesError::InvalidData)?;
        return Ok(());
    }
    let settings: serde_json::Value =
        serde_json::from_str(args.get(4).map_or("{}", String::as_str))
            .map_err(|_| GitNotesError::InvalidInput)?;
    let mut limits = GitNotesLimits::default();
    if let Some(value) = settings["max_note_bytes"].as_u64() {
        limits.max_note_bytes = value as usize;
    }
    if let Some(value) = settings["max_total_bytes"].as_u64() {
        limits.max_total_bytes = value as usize;
    }
    if let Some(value) = settings["max_listing_bytes"].as_u64() {
        limits.max_listing_bytes = value as usize;
    }
    if let Some(value) = settings["max_records"].as_u64() {
        limits.max_records = value as usize;
    }
    let selection = if let Some(ids) = settings.get("commits") {
        GitNoteSelection::Commits(
            serde_json::from_value(ids.clone()).map_err(|_| GitNotesError::InvalidInput)?,
        )
    } else {
        GitNoteSelection::All
    };
    let scope = GitNotesScope {
        repository,
        notes_ref: settings["notes_ref"]
            .as_str()
            .unwrap_or("refs/notes/ai")
            .into(),
        selection,
    };
    let collector = GitNotesCollector::new(loader, GitAiAdapter, OtlpJsonlWriter);
    if mode == "list" {
        let listing = collector.list_notes(&scope, limits)?;
        serde_json::to_writer(io::stdout().lock(), &listing)
            .map_err(|_| GitNotesError::InvalidData)?;
        return Ok(());
    }
    let request = GitNotesRequest {
        scope,
        limits,
        options: MappingOptions {
            include_content: mode == "content",
        },
    };
    if mode == "write-failure" || mode == "flush-failure" {
        let mut output = BrokenOutput {
            accepted: 0,
            fail_flush: mode == "flush-failure",
        };
        let result = collector.collect_notes(&request, &mut output);
        if result.is_ok() || output.accepted == 0 || (!output.fail_flush && output.accepted != 1) {
            return Err(GitNotesError::InvalidData);
        }
        result?;
    } else {
        collector.collect_notes(&request, &mut io::stdout().lock())?;
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let code = match &error {
                GitNotesError::Output(error) => Some(error.code()),
                _ => None,
            };
            eprintln!(
                "{}",
                serde_json::json!({"kind":error.kind(),"output_code":code})
            );
            ExitCode::FAILURE
        }
    }
}
