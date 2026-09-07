use std::io::{self, Write};

use unisphere_core::{Failure, FailureKind, InspectionReport};

use crate::args::Mode;

#[derive(Clone, Copy)]
pub(crate) enum Response<'a> {
    Report(&'a InspectionReport),
    Failure(&'a Failure),
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
        Response::Failure(failure) if failure.kind() == FailureKind::InvalidArguments => 2,
        Response::Failure(_) => 1,
        _ => 0,
    };
    let result = match mode {
        Mode::Json => json(response, stdout).and_then(|()| stdout.flush()),
        Mode::Human => {
            let writer = if matches!(response, Response::Failure(_)) {
                &mut *stderr
            } else {
                stdout
            };
            human(response, writer).and_then(|()| writer.flush())
        }
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
            writer.write_all(b"{\"ok\":true,\"command\":\"version\",\"v\":1,\"data\":{\"version\":")?;
            serde_json::to_writer(&mut *writer, version)?;
            writer.write_all(b"}}\n")
        }
        Response::Failure(failure) => {
            writer.write_all(b"{\"ok\":false,\"command\":\"config.check\",\"v\":1,\"error\":{\"kind\":")?;
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
    }
}

fn human(response: Response<'_>, writer: &mut dyn Write) -> io::Result<()> {
    match response {
        Response::Help(text) => writer.write_all(text.as_bytes()),
        Response::Version(version) => writeln!(writer, "unisphere {version}"),
        Response::Report(report) => {
            writeln!(writer, "Configuration valid.")?;
            writeln!(writer, "Source roots ({}):", report.configuration.source_roots.len())?;
            for root in &report.configuration.source_roots {
                writer.write_all(b"  ")?;
                // JSON string escaping preserves exact values while preventing
                // embedded newlines or terminal-control sequences from running.
                serde_json::to_writer(&mut *writer, root)?;
                writer.write_all(b"\n")?;
            }
            Ok(())
        }
        Response::Failure(failure) => {
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
