//! `unisphere prep`: render one incremental prep run through the injected port.
use std::io::Write;

use serde_json::json;
use unisphere_core::{
    PipelineError, PipelineErrorKind, ReadLimits, SnapshotLimits,
    prep::{PrepApi, PrepOptions, PrepReadLimits, PrepRequest, PrepSourceSet, PrepSourceStatus},
};

use crate::{PrepCommand, sessions::session_error};

/// Execute one parsed prep command over shell-resolved `roots`.
pub fn run_prep(
    command: &PrepCommand,
    roots: Vec<PrepSourceSet>,
    api: &dyn PrepApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let defaults = ReadLimits::default();
    let request = PrepRequest {
        target: command.target.clone(),
        roots,
        options: PrepOptions {
            include_content: command.include_content,
        },
        limits: PrepReadLimits {
            read: ReadLimits {
                max_records: usize::MAX,
                max_record_bytes: command
                    .max_record_bytes
                    .unwrap_or(defaults.max_record_bytes),
                max_batch_bytes: command.max_batch_bytes.unwrap_or(16 * 1024 * 1024),
            },
            snapshot: SnapshotLimits::default(),
        },
        threads: command.threads.unwrap_or(8),
        modified_since_ns: command.modified_since_ns,
    };
    let result = api.prep(&request).and_then(|report| {
        let unreadable: u64 = report
            .sets
            .iter()
            .filter_map(|set| set.by_status.get(PrepSourceStatus::Unreadable.label()))
            .sum();
        let value = json!({"ok": true, "command": "prep", "v": 1, "data": report,
            "next_action": {"summary": "Query TARGET/tables/*/*.parquet with a Parquet SQL engine; keep rows whose generation equals sources.generation.",
                "argv": ["unisphere", "prep", "--help"], "required_inputs": []}});
        serde_json::to_writer(&mut *stdout, &value)
            .ok()
            .and_then(|()| stdout.write_all(b"\n").ok())
            .ok_or_else(|| PipelineError::new(PipelineErrorKind::Write, None))?;
        Ok(unreadable)
    });
    match result {
        Ok(0) => 0,
        Ok(_) => 3,
        Err(error) => session_error(stderr, &error, 1),
    }
}
