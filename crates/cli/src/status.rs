//! `unisphere sessions status`: resolve each query to an explicit target through
//! the injected [`TargetResolver`], ask the injected [`SessionStatusApi`], and
//! render one result or failure per query. Status semantics belong to the ports.
use std::io::{self, Write};

use serde_json::{Value, json};
use unisphere_core::status::{
    ResolveBasis, Resolved, SessionStatus, SessionStatusApi, StatusFailure, StatusQuery,
    TargetResolver,
};

use crate::{OutputMode, SessionStatusCommand, query::safe_human_identifier};

/// Exit status when at least one query failed; each failure is in its result.
const EXIT_SOME_FAILED: u8 = 3;
const COMMAND: &str = "sessions.status";

enum Outcome {
    Status(Box<SessionStatus>),
    Failed {
        query: StatusQuery,
        resolved: Option<Box<Resolved>>,
        failure: StatusFailure,
    },
}

/// Execute one parsed `sessions status` at the caller-supplied `now_ms`.
///
/// Every query yields exactly one result, in argv order. `--session` queries
/// resolve explicitly without the resolver; `resolved.target.transcript` is the
/// transcript the status port read. Exit 0 when every query succeeded, 3 when at
/// least one failed, 1 when output could not be written.
pub fn run_status(
    command: &SessionStatusCommand,
    api: &dyn SessionStatusApi,
    resolver: &dyn TargetResolver,
    now_ms: i64,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let outcomes: Vec<Outcome> = command
        .queries
        .iter()
        .map(|query| status_of(query, api, resolver, now_ms))
        .collect();
    let failed = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Outcome::Failed { .. }))
        .count();
    let written = match command.mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => envelope(stdout, &outcomes, failed),
        OutputMode::Human => human(stdout, &outcomes),
    };
    if written.is_err() {
        let _ = stderr
            .write_all(b"unisphere: output incomplete; choose a healthy destination and retry.\n");
        let _ = stderr.flush();
        return 1;
    }
    if failed > 0 { EXIT_SOME_FAILED } else { 0 }
}

fn status_of(
    query: &StatusQuery,
    api: &dyn SessionStatusApi,
    resolver: &dyn TargetResolver,
    now_ms: i64,
) -> Outcome {
    let resolved = match query {
        StatusQuery::Target(target) => Ok(Resolved {
            query: query.clone(),
            target: target.clone(),
            pij_id: None,
            pane: None,
            basis: ResolveBasis::Explicit,
            conflicts: Vec::new(),
        }),
        StatusQuery::Pij(_) | StatusQuery::Pane(_) => resolver.resolve(query),
    };
    let mut resolved = match resolved {
        Ok(resolved) => resolved,
        Err(failure) => {
            return Outcome::Failed {
                query: query.clone(),
                resolved: None,
                failure,
            };
        }
    };
    match api.status(&resolved.target, now_ms) {
        Ok(mut status) => {
            resolved.target.transcript = status.target.transcript.clone();
            status.resolved = Some(resolved);
            Outcome::Status(Box::new(status))
        }
        Err(failure) => Outcome::Failed {
            query: query.clone(),
            resolved: Some(Box::new(resolved)),
            failure,
        },
    }
}

fn envelope(stdout: &mut dyn Write, outcomes: &[Outcome], failed: usize) -> io::Result<()> {
    let results = outcomes
        .iter()
        .map(|outcome| match outcome {
            Outcome::Status(status) => Ok(json!({
                "ok": true,
                "query": status.resolved.as_ref().map(|resolved| &resolved.query),
                "status": serde_json::to_value(status)?,
            })),
            Outcome::Failed {
                query,
                resolved,
                failure,
            } => Ok(json!({
                "ok": false,
                "query": query,
                "resolved": resolved,
                "error": {"kind": failure.kind, "code": failure.code(),
                    "message": failure.message, "fix": failure.recovery()},
            })),
        })
        .collect::<serde_json::Result<Vec<Value>>>()
        .map_err(io::Error::other)?;
    let summary = if failed > 0 {
        format!(
            "{failed} of {} queries failed; each failed result names its error code and fix.",
            outcomes.len()
        )
    } else {
        "Every fact names its basis; facts listed in `unknown` are not recorded by the harness."
            .to_owned()
    };
    serde_json::to_writer(
        &mut *stdout,
        &json!({"ok": true, "command": COMMAND, "v": 1,
            "data": {"results": results, "failed": failed},
            "next_action": {"summary": summary,
                "argv": ["unisphere", "docs", "get", "session-status", "--human"],
                "required_inputs": []}}),
    )
    .map_err(io::Error::other)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

const HEADERS: [&str; 9] = [
    "QUERY", "HARNESS", "SESSION", "MODEL", "CONTEXT", "IDLE", "TURNS", "COMPACT", "CACHE",
];

/// One table row per query; conflicts and failures follow the table.
fn human(out: &mut dyn Write, outcomes: &[Outcome]) -> io::Result<()> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for outcome in outcomes {
        match outcome {
            Outcome::Status(status) => {
                let resolved = status.resolved.as_ref();
                let label = resolved.map_or_else(|| "session".to_owned(), |r| label(&r.query));
                for conflict in resolved.into_iter().flat_map(|r| &r.conflicts) {
                    notes.push(format!(
                        "{label}: conflict ({}): {} {}",
                        basis(conflict.basis),
                        safe(&conflict.target.harness),
                        safe(&conflict.target.session_id)
                    ));
                }
                rows.push(row(&label, status));
            }
            Outcome::Failed { query, failure, .. } => {
                let label = label(query);
                notes.push(format!(
                    "{label}: {}: {}\n  Next: {}",
                    failure.code(),
                    safe(&failure.message),
                    failure.recovery()
                ));
                let mut row = vec![label, failure.code().to_owned()];
                row.resize(HEADERS.len(), "-".to_owned());
                rows.push(row);
            }
        }
    }
    let mut widths = HEADERS.map(str::len);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |out: &mut dyn Write, cells: &[&str]| -> io::Result<()> {
        let mut text = String::new();
        for (index, (cell, width)) in cells.iter().zip(widths).enumerate() {
            if index + 1 == cells.len() {
                text.push_str(cell);
            } else {
                text.push_str(&format!("{cell:<width$}  "));
            }
        }
        writeln!(out, "{}", text.trim_end())
    };
    line(out, &HEADERS)?;
    for row in &rows {
        line(out, &row.iter().map(String::as_str).collect::<Vec<_>>())?;
    }
    for note in notes {
        writeln!(out, "{note}")?;
    }
    writeln!(out, "Guide: unisphere docs get session-status --human")?;
    out.flush()
}

fn row(label: &str, status: &SessionStatus) -> Vec<String> {
    let unknown = || "?".to_owned();
    let model = match (&status.model.current, &status.model.pending_switch) {
        (Some(current), Some(pending)) => {
            format!(
                "{} -> {} (pending)",
                safe(&current.value),
                safe(&pending.requested)
            )
        }
        (Some(current), None) => safe(&current.value),
        (None, Some(pending)) => format!("? -> {} (pending)", safe(&pending.requested)),
        (None, None) => unknown(),
    };
    let context = status
        .context
        .display
        .as_deref()
        .map(safe)
        .or_else(|| {
            status
                .context
                .used_tokens
                .as_ref()
                .map(|used| format!("{} used", used.value))
        })
        .unwrap_or_else(unknown);
    let idle = status
        .timeline
        .idle_seconds
        .map(duration)
        .unwrap_or_else(unknown);
    let turns = format!(
        "{} ({}/1h)",
        status.turns.total, status.turns.last_hour_total
    );
    let compact = status.compaction.counts.map_or_else(unknown, |counts| {
        (counts.manual + counts.auto + counts.unknown_trigger).to_string()
    });
    let cache = status
        .last_call
        .as_ref()
        .map(|call| {
            let bucket = call.ttl_bucket.as_ref().map(|bucket| safe(&bucket.value));
            match (call.cache_warm.as_ref().map(|warm| warm.value), bucket) {
                (Some(true), Some(bucket)) => format!("warm {bucket}"),
                (Some(false), Some(bucket)) => format!("cold {bucket}"),
                (Some(true), None) => "warm".to_owned(),
                (Some(false), None) => "cold".to_owned(),
                (None, Some(bucket)) => bucket,
                (None, None) => unknown(),
            }
        })
        .unwrap_or_else(unknown);
    vec![
        label.to_owned(),
        safe(&status.target.harness),
        safe(&status.target.session_id),
        model,
        context,
        idle,
        turns,
        compact,
        cache,
    ]
}

fn label(query: &StatusQuery) -> String {
    match query {
        StatusQuery::Pij(id) => safe(id),
        StatusQuery::Pane(pane) => safe(pane),
        StatusQuery::Target(target) => safe(&target.session_id),
    }
}

const fn basis(basis: ResolveBasis) -> &'static str {
    match basis {
        ResolveBasis::Explicit => "explicit",
        ResolveBasis::PijRegistry => "pij_registry",
        ResolveBasis::NativePane => "native_pane",
    }
}

fn duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m{:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h{:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

fn safe(value: &str) -> String {
    safe_human_identifier(value)
}
