use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use unisphere_core::{AdapterDescriptor, Failure, FailureKind, InspectionReport};

use crate::args::OutputMode;

#[derive(Clone, Copy)]
pub(crate) enum Response<'a> {
    Report(&'a InspectionReport),
    Failure(&'a Failure),
    Catalog(&'a [&'a AdapterDescriptor]),
    CatalogFailure(&'a Failure),
    Help(&'a str),
    Version(&'a str),
}

pub(crate) fn emit(
    response: Response<'_>,
    mode: OutputMode,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let code = match response {
        Response::Failure(failure) | Response::CatalogFailure(failure)
            if failure.kind() == FailureKind::InvalidArguments =>
        {
            2
        }
        Response::Failure(_) | Response::CatalogFailure(_) => 1,
        _ => 0,
    };
    let result = match mode {
        OutputMode::Json => json(response, stdout).and_then(|()| stdout.flush()),
        OutputMode::JsonDiagnostic
            if matches!(response, Response::Failure(_) | Response::CatalogFailure(_)) =>
        {
            json(response, stderr).and_then(|()| stderr.flush())
        }
        OutputMode::JsonDiagnostic => json(response, stdout).and_then(|()| stdout.flush()),
        OutputMode::Human
            if matches!(response, Response::Failure(_) | Response::CatalogFailure(_)) =>
        {
            human(response, stderr).and_then(|()| stderr.flush())
        }
        OutputMode::Human => human(response, stdout).and_then(|()| stdout.flush()),
    };
    if result.is_err() {
        let _ = stderr
            .write_all(b"unisphere: output incomplete; choose a healthy destination and retry.\n");
        let _ = stderr.flush();
        1
    } else {
        code
    }
}

fn json(response: Response<'_>, writer: &mut dyn Write) -> io::Result<()> {
    match response {
        Response::Catalog(adapters) => {
            writer.write_all(
                b"{\"ok\":true,\"command\":\"adapters.list\",\"v\":1,\"data\":{\"adapters\":",
            )?;
            serde_json::to_writer(&mut *writer, adapters)?;
            writer.write_all(b"},\"next_action\":")?;
            write_action(
                writer,
                "Choose a registered adapter, then read the source-discovery workflow.",
                &["unisphere", "docs", "get", "find-sessions", "--human"],
                &[],
            )?;
            writer.write_all(b"}\n")
        }
        Response::Report(report) => {
            writer.write_all(b"{\"ok\":true,\"command\":\"config.check\",\"v\":1,\"data\":")?;
            serde_json::to_writer(&mut *writer, report)?;
            writer.write_all(b",\"next_action\":")?;
            write_action(
                writer,
                "Choose the intended repository, then inspect its source coverage.",
                &["unisphere", "sources", "list", "--repo"],
                &["repository_path"],
            )?;
            writer.write_all(b"}\n")
        }
        Response::Help(text) => {
            writer.write_all(b"{\"ok\":true,\"command\":\"help\",\"v\":1,\"data\":{\"text\":")?;
            serde_json::to_writer(&mut *writer, text)?;
            writer.write_all(b"},\"next_action\":")?;
            write_action(
                writer,
                "Read the bundled start workflow.",
                &["unisphere", "docs", "get", "start", "--human"],
                &[],
            )?;
            writer.write_all(b"}\n")
        }
        Response::Version(version) => {
            writer.write_all(
                b"{\"ok\":true,\"command\":\"version\",\"v\":1,\"data\":{\"version\":",
            )?;
            serde_json::to_writer(&mut *writer, version)?;
            writer.write_all(b"},\"next_action\":")?;
            write_action(
                writer,
                "Inspect this version's command grammar.",
                &["unisphere", "--help"],
                &[],
            )?;
            writer.write_all(b"}\n")
        }
        Response::Failure(failure) => json_failure(writer, "config.check", failure),
        Response::CatalogFailure(failure) => json_failure(writer, "adapters.list", failure),
    }
}

fn json_failure(writer: &mut dyn Write, command: &str, failure: &Failure) -> io::Result<()> {
    writer.write_all(b"{\"ok\":false,\"command\":")?;
    serde_json::to_writer(&mut *writer, command)?;
    writer.write_all(b",\"v\":1,\"error\":{\"kind\":")?;
    serde_json::to_writer(&mut *writer, &failure.kind())?;
    writer.write_all(b",\"code\":")?;
    serde_json::to_writer(&mut *writer, failure.code())?;
    writer.write_all(b",\"message\":")?;
    serde_json::to_writer(&mut *writer, failure.message())?;
    writer.write_all(b",\"fix\":")?;
    serde_json::to_writer(&mut *writer, failure.fix())?;
    writer.write_all(b",\"retryable\":")?;
    serde_json::to_writer(&mut *writer, &failure.retryable())?;
    writer.write_all(b",\"location\":")?;
    serde_json::to_writer(&mut *writer, &failure.location())?;
    writer.write_all(b"},\"next_action\":")?;
    write_action(writer, failure.fix(), &["unisphere", "--help"], &[])?;
    writer.write_all(b"}\n")
}

fn write_action(
    writer: &mut dyn Write,
    summary: &str,
    argv: &[&str],
    required_inputs: &[&str],
) -> io::Result<()> {
    serde_json::to_writer(
        writer,
        &serde_json::json!({
            "summary": summary,
            "argv": argv,
            "required_inputs": required_inputs,
        }),
    )
    .map_err(io::Error::other)
}

fn human(response: Response<'_>, writer: &mut dyn Write) -> io::Result<()> {
    match response {
        Response::Catalog(adapters) => {
            writeln!(
                writer,
                "Registered adapters; location hints are not detected installations:"
            )?;
            for adapter in adapters {
                writeln!(writer, "{:?} ({:?})", adapter.id, adapter.application)?;
                writeln!(writer, "  {:?}", adapter.description)?;
                for hint in adapter.locations {
                    writeln!(
                        writer,
                        "  Location hint: {:?}/{:?}; pattern {:?}; {:?} on {:?}",
                        hint.base,
                        hint.path,
                        hint.session_glob,
                        hint.storage_format,
                        hint.platforms
                    )?;
                }
                let capabilities = &adapter.capabilities;
                writeln!(
                    writer,
                    "  Export: {:?} on {:?}",
                    capabilities.output_formats, capabilities.export_platforms
                )?;
                writeln!(
                    writer,
                    "  Caller-owned SDK cursor: {}; source assumption: {:?}",
                    capabilities.sdk_caller_owned_cursor, capabilities.cursor_source_assumption
                )?;
                writeln!(
                    writer,
                    "  Persisted CLI resume: {}; revision reconciliation: {}; lossless archive: {}",
                    capabilities.cli_persisted_resume,
                    capabilities.delayed_revision_reconciliation,
                    capabilities.lossless_archive
                )?;
            }
            writeln!(
                writer,
                "Next: read `unisphere docs get find-sessions --human`."
            )
        }
        Response::Help(text) => {
            writer.write_all(text.as_bytes())?;
            writeln!(writer, "Next: run `unisphere docs get start --human`.")
        }
        Response::Version(version) => {
            writeln!(writer, "unisphere {version}")?;
            writeln!(writer, "Next: run `unisphere --help`.")
        }
        Response::Report(report) => {
            writeln!(writer, "Configuration valid.")?;
            writeln!(
                writer,
                "Source roots ({}):",
                report.configuration.source_roots.len()
            )?;
            for root in &report.configuration.source_roots {
                writer.write_all(b"  ")?;
                serde_json::to_writer(&mut *writer, root)?;
                writer.write_all(b"\n")?;
            }
            writeln!(
                writer,
                "Next: run `unisphere sources list --repo PATH --format json`."
            )
        }
        Response::Failure(failure) | Response::CatalogFailure(failure) => {
            writeln!(writer, "{}: {}", failure.code(), failure.message())?;
            if let Some(location) = failure.location() {
                if let Some(path) = &location.path {
                    writer.write_all(b"Path: ")?;
                    serde_json::to_writer(&mut *writer, path)?;
                    writer.write_all(b"\n")?;
                }
                if let Some(field) = &location.field {
                    writer.write_all(b"Field: ")?;
                    serde_json::to_writer(&mut *writer, field)?;
                    writer.write_all(b"\n")?;
                }
                if let Some(line) = location.line {
                    writeln!(writer, "Line: {line}")?;
                }
                if let Some(column) = location.column {
                    writeln!(writer, "Column: {column}")?;
                }
            }
            writeln!(writer, "Fix: {}", failure.fix())?;
            writeln!(writer, "Next: {}", failure.fix())
        }
    }
}

static STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct StagedOutput {
    target: PathBuf,
    stage: PathBuf,
    file: Option<File>,
}

fn open_stage(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

impl StagedOutput {
    pub(crate) fn preflight(target: &Path) -> io::Result<()> {
        match std::fs::symlink_metadata(target) {
            Ok(_) => return Err(io::ErrorKind::AlreadyExists.into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let parent = target.parent().ok_or(io::ErrorKind::InvalidInput)?;
        let metadata = parent.metadata()?;
        if !metadata.is_dir() {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        Ok(())
    }
    pub(crate) fn create(target: &Path) -> io::Result<Self> {
        Self::preflight(target)?;
        let parent = target.parent().ok_or(io::ErrorKind::InvalidInput)?;
        for _ in 0..32 {
            let sequence = STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let stage = parent.join(format!(
                ".unisphere-output-{}-{sequence}.tmp",
                std::process::id()
            ));
            match open_stage(&stage) {
                Ok(file) => {
                    return Ok(Self {
                        target: target.to_owned(),
                        stage,
                        file: Some(file),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::ErrorKind::AlreadyExists.into())
    }

    pub(crate) fn writer(&mut self) -> &mut File {
        self.file.as_mut().expect("staged output is not published")
    }

    pub(crate) fn publish(mut self) -> io::Result<()> {
        self.writer().flush()?;
        drop(self.file.take());
        std::fs::hard_link(&self.stage, &self.target)?;
        let _ = std::fs::remove_file(&self.stage);
        Ok(())
    }
}

impl Drop for StagedOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.stage);
    }
}
