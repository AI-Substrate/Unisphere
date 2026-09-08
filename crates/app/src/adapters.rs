//! Explicit compile-time adapter registration; no plugin loader or service locator.
use std::{ffi::OsString, io::Write};
use unisphere_cli::CliContext;
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_output_otlp::OtlpJsonlWriter;
use unisphere_sdk::{Collector, PipelineError, PipelineErrorKind, SessionAdapter};

struct AdapterRegistration {
    name: &'static str,
    run: fn(Vec<OsString>, &CliContext, &mut dyn Write, &mut dyn Write) -> u8,
}

const ADAPTERS: &[AdapterRegistration] = &[AdapterRegistration {
    name: "claude-code",
    run: |args, context, stdout, stderr| {
        run_with_adapter(
            unisphere_adapter_claude::ClaudeCodeAdapter,
            args,
            context,
            stdout,
            stderr,
        )
    },
}];

fn run_with_adapter<A: SessionAdapter>(
    adapter: A,
    args: Vec<OsString>,
    context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let collector = Collector::new(FileSessionLoader, adapter, OtlpJsonlWriter);
    unisphere_cli::run_sessions(args, context, &collector, stdout, stderr)
}

fn dispatch(
    registry: &[AdapterRegistration],
    args: Vec<OsString>,
    context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let name = unisphere_cli::requested_session_adapter(&args);
    match registry.iter().find(|entry| entry.name == name) {
        Some(entry) => (entry.run)(args, context, stdout, stderr),
        None => unisphere_cli::session_error(
            stderr,
            &PipelineError::new(PipelineErrorKind::InvalidInput, None),
            2,
        ),
    }
}

pub fn run(
    args: Vec<OsString>,
    context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    dispatch(ADAPTERS, args, context, stdout, stderr)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use unisphere_testkit::collection::TextFixtureAdapter;

    #[test]
    fn another_adapter_needs_one_registration_and_no_core_or_claude_change() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("session.jsonl"),
            b"SENSITIVE-CUSTOM-TEXT\n",
        )
        .unwrap();
        let registry = [AdapterRegistration {
            name: "fixture-text",
            run: |args, context, stdout, stderr| {
                run_with_adapter(TextFixtureAdapter, args, context, stdout, stderr)
            },
        }];
        let context = CliContext {
            cwd: root.path().into(),
            stdout_is_terminal: false,
            version: "test".into(),
        };
        let args = [
            "unisphere",
            "sessions",
            "export",
            "--adapter",
            "fixture-text",
            "--input",
            "session.jsonl",
            "--include-content",
        ]
        .map(OsString::from)
        .to_vec();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            dispatch(&registry, args, &context, &mut stdout, &mut stderr),
            0
        );
        let document: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        let record = &document["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert!(
            record["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|attr| attr["key"] == "unisphere.source.adapter"
                    && attr["value"]["stringValue"] == "fixture-text")
        );
        assert!(
            String::from_utf8(stdout)
                .unwrap()
                .contains("SENSITIVE-CUSTOM-TEXT")
        );
    }
}
