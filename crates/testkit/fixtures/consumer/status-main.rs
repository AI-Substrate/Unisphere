#![forbid(unsafe_code)]
//! External SDK consumer for session status: embeds StatusService in-process
//! with a caller-held cursor, as an embedding client (e.g. pij) would.
//! Usage: unisphere-status-consumer <claude-root> <session-id> <append-record>

use serde_json::{Value, json};
use std::{fs, io::Write, path::PathBuf, process::ExitCode, sync::Arc};
use unisphere_sdk::{
    prep::{PrepBinding, default_set},
    status::{SessionStatus, SessionStatusApi, StatusService, StatusTarget},
};

const NOW_MS: i64 = 1_800_000_000_000;

/// Facts that must agree between cold, incremental and CLI answers.
fn project(status: &SessionStatus) -> Value {
    json!({
        "target": status.target.session_id,
        "model": status.model.current.as_ref().map(|fact| &fact.value),
        "used_tokens": status.context.used_tokens.as_ref().map(|fact| fact.value),
        "calls": status.calls.total,
        "turns": status.turns.total,
        "compactions": status.compaction.counts,
        "unknown": status.unknown,
    })
}

fn run() -> Result<Value, String> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("missing root")?);
    let session = args.next().ok_or("missing session id")?;
    let appended = args.next().ok_or("missing append record")?;
    let service = StatusService::new(
        vec![PrepBinding {
            fold: Arc::new(unisphere_adapter_claude::ClaudePrepFold),
            loader: Arc::new(unisphere_loader_jsonl::FileSessionLoader),
        }],
        vec![default_set("claude-code", root)],
    );
    let target = StatusTarget {
        harness: "claude-code".into(),
        session_id: session,
        transcript: None,
    };
    let fail = |e: unisphere_sdk::status::StatusFailure| e.to_string();
    let (cold, cursor) = service.status_incremental(&target, None, NOW_MS).map_err(fail)?;
    let (unchanged, cursor) = service
        .status_incremental(&target, Some(&cursor), NOW_MS)
        .map_err(fail)?;
    let path = cold.target.transcript.clone().ok_or("no transcript path")?;
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(format!("{appended}\n").as_bytes()))
        .map_err(|e| e.to_string())?;
    let (incremental, _) = service
        .status_incremental(&target, Some(&cursor), NOW_MS)
        .map_err(fail)?;
    let cold_after = service.status(&target, NOW_MS).map_err(fail)?;
    Ok(json!({
        "cold": project(&cold),
        "unchanged_bytes_read": unchanged.source.bytes_read,
        "incremental": project(&incremental),
        "incremental_bytes_read": incremental.source.bytes_read,
        "cold_after": project(&cold_after),
        "cold_after_bytes_read": cold_after.source.bytes_read,
    }))
}

fn main() -> ExitCode {
    match run() {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
