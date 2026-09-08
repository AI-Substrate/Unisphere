use std::io::{self, Write};

use unisphere_core::{AdapterDescriptor, Failure, FailureKind, InspectionReport};

use crate::args::Mode;

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
    mode: Mode,
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
        Mode::Json => json(response, stdout).and_then(|()| stdout.flush()),
        Mode::Human if matches!(response, Response::Failure(_) | Response::CatalogFailure(_)) => {
            human(response, stderr).and_then(|()| stderr.flush())
        }
        Mode::Human => human(response, stdout).and_then(|()| stdout.flush()),
    };
    if result.is_err() {
        // The failed writer may already contain a prefix. Do not append a second
        // machine envelope or expose raw I/O diagnostics. A broken stderr is OK.
        let _ = stderr.write_all(b"unisphere: could not write output.\n");
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
            writer.write_all(b"}}\n")
        }
        Response::Report(report) => {
            writer.write_all(b"{\"ok\":true,\"command\":\"config.check\",\"v\":1,\"data\":")?;
            serde_json::to_writer(&mut *writer, report)?;
            writer.write_all(b"}\n")
        }
        Response::Help(text) => {
            writer.write_all(b"{\"ok\":true,\"command\":\"help\",\"v\":1,\"data\":{\"text\":")?;
            serde_json::to_writer(&mut *writer, text)?;
            writer.write_all(b"}}\n")
        }
        Response::Version(version) => {
            writer.write_all(
                b"{\"ok\":true,\"command\":\"version\",\"v\":1,\"data\":{\"version\":",
            )?;
            serde_json::to_writer(&mut *writer, version)?;
            writer.write_all(b"}}\n")
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
    writer.write_all(b"}}\n")
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
            Ok(())
        }
        Response::Help(text) => writer.write_all(text.as_bytes()),
        Response::Version(version) => writeln!(writer, "unisphere {version}"),
        Response::Report(report) => {
            writeln!(writer, "Configuration valid.")?;
            writeln!(
                writer,
                "Source roots ({}):",
                report.configuration.source_roots.len()
            )?;
            for root in &report.configuration.source_roots {
                writer.write_all(b"  ")?;
                // JSON string escaping preserves exact values while preventing
                // embedded newlines or terminal-control sequences from running.
                serde_json::to_writer(&mut *writer, root)?;
                writer.write_all(b"\n")?;
            }
            Ok(())
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
            writeln!(writer, "Fix: {}", failure.fix())
        }
    }
}
