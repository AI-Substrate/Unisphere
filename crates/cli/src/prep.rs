//! `unisphere prep`: turn parsed prep commands into [`PrepApi`] requests and
//! render the returned reports. Every prep decision (roots, change detection,
//! generations, coverage, content gating) belongs to the injected port.
use std::io::{self, Write};

use serde_json::{Value, json};
use unisphere_core::{
    PipelineError, PipelineErrorKind,
    prep::{
        PrepApi, PrepCompactReport, PrepRecord, PrepReport, PrepSetReport, PrepSourceOutcome,
        PrepSourceSet, PrepSourceStatus, PrepTableCounts,
    },
};

use crate::{
    OutputMode, PrepCommand, PrepCompactCommand, PrepRecordCommand, query::safe_human_identifier,
};

/// Exit status when the run committed but at least one source was unreadable.
const EXIT_UNREADABLE: u8 = 3;
/// Human output lists at most this many attention sources; JSON lists all.
const HUMAN_SOURCE_LINES: usize = 20;
/// Display order of the status vocabulary.
const STATUS_ORDER: [&str; 8] = [
    "new",
    "appended",
    "replaced",
    "unchanged",
    "skipped",
    "missing",
    "unreadable",
    "unsupported",
];

/// Execute one parsed prep run over shell-resolved `roots` (catalogue defaults
/// merged with [`PrepCommand::explicit_sets`] by the composition root).
///
/// Exit 0 when every source was read or intentionally skipped, 3 when the run
/// committed but a source was unreadable, 2 when the port rejects the request as
/// invalid input and 1 for any other failure.
pub fn run_prep(
    command: &PrepCommand,
    roots: Vec<PrepSourceSet>,
    api: &dyn PrepApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let report = match api.prep(&command.request(roots)) {
        Ok(report) => report,
        Err(error) => return failure("prep", &error, command.mode, stdout, stderr),
    };
    let unreadable = unreadable_sources(&report);
    let written = match command.mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            let action = if unreadable > 0 {
                json!({"summary": format!("{unreadable} source(s) were unreadable; their previously committed rows and state are kept. Restore read access, then re-run the same prep command."),
                    "argv": ["unisphere", "prep", "--target"], "required_inputs": ["target"]})
            } else {
                docs_action(
                    "Load TARGET/views.sql in DuckDB and query the canonical views (calls_v, turns_v, triggers_v, events_v, compactions_v, tool_uses_v, sources_v, sessions_v); re-run prep to pick up appended records.",
                )
            };
            envelope(stdout, "prep", serde_json::to_value(&report), action)
        }
        OutputMode::Human => human_report(stdout, &report, unreadable),
    };
    finish(
        written,
        if unreadable > 0 { EXIT_UNREADABLE } else { 0 },
        stderr,
    )
}

/// Execute one parsed `prep compact` through the injected port.
pub fn run_prep_compact(
    command: &PrepCompactCommand,
    api: &dyn PrepApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let report = match api.compact(&command.request()) {
        Ok(report) => report,
        Err(error) => return failure("prep.compact", &error, command.mode, stdout, stderr),
    };
    let written = match command.mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => envelope(
            stdout,
            "prep.compact",
            serde_json::to_value(&report),
            docs_action(
                "Canonical view results are unchanged; query them through TARGET/views.sql.",
            ),
        ),
        OutputMode::Human => human_compact(stdout, &report),
    };
    finish(written, 0, stderr)
}

/// Execute one parsed `prep record`. Without the content opt-in the command is
/// refused (exit 2) before the port is called.
pub fn run_prep_record(
    command: &PrepRecordCommand,
    api: &dyn PrepApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    if !command.include_content {
        let error = PipelineError::new(PipelineErrorKind::InvalidInput, None);
        return failure("prep.record", &error, command.mode, stdout, stderr);
    }
    let record = match api.record(&command.request()) {
        Ok(record) => record,
        Err(error) => return failure("prep.record", &error, command.mode, stdout, stderr),
    };
    let written = match command.mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            let (encoding, text) = record_text(&record.bytes);
            envelope(
                stdout,
                "prep.record",
                Ok(
                    json!({"source": record.source, "path": record.path, "address": record.address,
                    "bytes": record.bytes.len(), "encoding": encoding, "record": text}),
                ),
                docs_action(
                    "This is one native record, emitted because --include-content was given; prepped tables remain metadata-only.",
                ),
            )
        }
        OutputMode::Human => human_record(stdout, stderr, &record),
    };
    finish(written, 0, stderr)
}

fn unreadable_sources(report: &PrepReport) -> u64 {
    let counted: u64 = report
        .sets
        .iter()
        .filter_map(|set| set.by_status.get(PrepSourceStatus::Unreadable.label()))
        .sum();
    let listed = report
        .sources
        .iter()
        .filter(|source| source.status == PrepSourceStatus::Unreadable)
        .count() as u64;
    counted.max(listed)
}

fn docs_action(summary: &str) -> Value {
    json!({"summary": summary, "argv": ["unisphere", "docs", "get", "prep", "--human"], "required_inputs": []})
}

fn envelope(
    stdout: &mut dyn Write,
    command: &str,
    data: serde_json::Result<Value>,
    next_action: Value,
) -> io::Result<()> {
    let data = data.map_err(io::Error::other)?;
    serde_json::to_writer(
        &mut *stdout,
        &json!({"ok": true, "command": command, "v": 1, "data": data, "next_action": next_action}),
    )
    .map_err(io::Error::other)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn finish(written: io::Result<()>, exit: u8, stderr: &mut dyn Write) -> u8 {
    if written.is_ok() {
        return exit;
    }
    let _ = stderr
        .write_all(b"unisphere: output incomplete; choose a healthy destination and retry.\n");
    let _ = stderr.flush();
    1
}

/// Safe diagnostic: fixed code/message/fix, no argument values or source payloads.
fn failure(
    command: &str,
    error: &PipelineError,
    mode: OutputMode,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let refused = error.kind() == PipelineErrorKind::InvalidInput;
    let exit = if refused { 2 } else { 1 };
    let (fix, argv) = if refused && command == "prep.record" {
        (
            "Re-run `unisphere prep record` with --include-content only if emitting that native record is intended.",
            vec!["unisphere", "prep", "record", "--help"],
        )
    } else {
        (error.fix(), vec!["unisphere", "prep", "--help"])
    };
    let result = match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            let destination: &mut dyn Write = if mode == OutputMode::Json {
                stdout
            } else {
                stderr
            };
            serde_json::to_writer(
                &mut *destination,
                &json!({"ok": false, "command": command, "v": 1,
                    "error": {"kind": error.kind(), "code": error.code(), "message": error.message(),
                        "fix": fix, "offset": error.offset(), "retryable": false},
                    "next_action": {"summary": fix, "argv": argv, "required_inputs": []}}),
            )
            .map_err(io::Error::other)
            .and_then(|()| destination.write_all(b"\n"))
            .and_then(|()| destination.flush())
        }
        OutputMode::Human => writeln!(stderr, "{}: {}\nNext: {fix}", error.code(), error.message())
            .and_then(|()| stderr.flush()),
    };
    if result.is_ok() { exit } else { 1 }
}

fn human_report(out: &mut dyn Write, report: &PrepReport, unreadable: u64) -> io::Result<()> {
    writeln!(
        out,
        "Prepped {} (run {}, table schema v{}).",
        safe(&report.target.to_string_lossy()),
        report.run,
        report.table_schema_version
    )?;
    for set in &report.sets {
        human_set(out, set)?;
    }
    writeln!(
        out,
        "Read {} native bytes; rows written: {}; {} part(s) committed in {} commit(s).",
        report.bytes_read,
        table_counts(&report.rows_written),
        report.commit.parts_written.len(),
        report.commits
    )?;
    let tails: Vec<&PrepSourceOutcome> = report
        .sources
        .iter()
        .filter(|source| source.pending_tail_bytes > 0)
        .collect();
    if report.pending_tail_bytes > 0 {
        writeln!(
            out,
            "Pending tails: {} byte(s) after the last complete record in {} source(s); read by a later run once complete.",
            report.pending_tail_bytes,
            tails.len()
        )?;
    }
    let attention: Vec<&PrepSourceOutcome> = report
        .sources
        .iter()
        .filter(|source| {
            !matches!(
                source.status,
                PrepSourceStatus::New | PrepSourceStatus::Appended
            ) || source.pending_tail_bytes > 0
        })
        .collect();
    if !attention.is_empty() {
        writeln!(out, "Sources needing attention:")?;
        for source in attention.iter().take(HUMAN_SOURCE_LINES) {
            human_source(out, source)?;
        }
        if attention.len() > HUMAN_SOURCE_LINES {
            writeln!(
                out,
                "  … {} more; --json lists every source outcome.",
                attention.len() - HUMAN_SOURCE_LINES
            )?;
        }
    }
    if unreadable > 0 {
        writeln!(
            out,
            "Next: restore read access to the {unreadable} unreadable source(s) (their committed rows and state are kept), then re-run the same prep command."
        )?;
    } else {
        writeln!(
            out,
            "Next: load TARGET/views.sql in DuckDB and query the canonical views; see `unisphere docs get prep --human`."
        )?;
    }
    out.flush()
}

fn human_set(out: &mut dyn Write, set: &PrepSetReport) -> io::Result<()> {
    writeln!(
        out,
        "{}/{}  {}",
        safe(&set.harness),
        safe(&set.label),
        safe(&set.root.to_string_lossy())
    )?;
    let statuses = STATUS_ORDER
        .iter()
        .filter_map(|status| {
            set.by_status
                .get(*status)
                .filter(|count| **count > 0)
                .map(|count| format!("{count} {status}"))
        })
        .collect::<Vec<_>>();
    let statuses = if statuses.is_empty() {
        "none".to_owned()
    } else {
        statuses.join(", ")
    };
    if set.supported {
        writeln!(
            out,
            "  policy {}; discovered {}: {statuses}",
            set.policy.as_deref().map_or_else(|| "none".into(), safe),
            set.discovered
        )?;
    } else {
        writeln!(
            out,
            "  unsupported: no prep binding for this harness; discovered {}: {statuses}; nothing read",
            set.discovered
        )?;
    }
    writeln!(
        out,
        "  discovery skipped: symlinks {}, hidden {}, unreadable entries {}; directories listed {}, reused {}",
        set.skipped.symlinks,
        set.skipped.hidden,
        set.skipped.unreadable_entries,
        set.dirs_listed,
        set.dirs_reused
    )
}

fn human_source(out: &mut dyn Write, source: &PrepSourceOutcome) -> io::Result<()> {
    let status = match source.status {
        PrepSourceStatus::Replaced { reason } => format!(
            "replaced ({})",
            serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        ),
        status => status.label().to_owned(),
    };
    write!(
        out,
        "  {status:<22} {}  generation {}",
        safe(&source.source),
        source.generation
    )?;
    if source.pending_tail_bytes > 0 {
        write!(out, "; pending tail {} byte(s)", source.pending_tail_bytes)?;
    }
    if let Some(error) = &source.error {
        write!(out, "; {}", safe(error))?;
    }
    writeln!(out)
}

fn human_compact(out: &mut dyn Write, report: &PrepCompactReport) -> io::Result<()> {
    writeln!(
        out,
        "Compacted {}: parts {} → {}, bytes {} → {}.",
        safe(&report.target.to_string_lossy()),
        report.parts_before,
        report.parts_after,
        report.bytes_before,
        report.bytes_after
    )?;
    writeln!(out, "Rows before: {}", table_counts(&report.rows_before))?;
    writeln!(out, "Rows after:  {}", table_counts(&report.rows_after))?;
    writeln!(
        out,
        "Next: canonical view results are unchanged; query them through TARGET/views.sql."
    )?;
    out.flush()
}

fn human_record(
    out: &mut dyn Write,
    stderr: &mut dyn Write,
    record: &PrepRecord,
) -> io::Result<()> {
    out.write_all(&record.bytes)?;
    if !record.bytes.ends_with(b"\n") {
        out.write_all(b"\n")?;
    }
    out.flush()?;
    writeln!(
        stderr,
        "Next: this native record ({} bytes) was emitted because --include-content was given; prepped tables remain metadata-only.",
        record.bytes.len()
    )?;
    stderr.flush()
}

fn table_counts(counts: &PrepTableCounts) -> String {
    format!(
        "calls {}, turns {}, triggers {}, events {}, tool_uses {}",
        counts.calls, counts.turns, counts.triggers, counts.events, counts.tool_uses
    )
}

/// UTF-8 records are returned verbatim; anything else as lowercase hex.
fn record_text(bytes: &[u8]) -> (&'static str, String) {
    match std::str::from_utf8(bytes) {
        Ok(text) => ("utf8", text.to_owned()),
        Err(_) => (
            "hex",
            bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        ),
    }
}

fn safe(value: &str) -> String {
    safe_human_identifier(value)
}
