//! Explicit compile-time adapter registration; no plugin loader or service locator.
use std::{ffi::OsString, io::Write};
use unisphere_cli::{AdapterCapabilities, AdapterDescriptor, CliContext, LocationHint};
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_output_otlp::OtlpJsonlWriter;
use unisphere_sdk::{Collector, PipelineError, PipelineErrorKind, SessionAdapter};

struct AdapterRegistration {
    descriptor: AdapterDescriptor,
    run: fn(Vec<OsString>, &CliContext, &mut dyn Write, &mut dyn Write) -> u8,
}

const ADAPTERS: [AdapterRegistration; 1] = [AdapterRegistration {
    descriptor: AdapterDescriptor {
        id: "claude-code",
        application: "Claude Code",
        description: "Source-derived Claude Code JSONL projection; metadata by default, content by explicit opt-in.",
        locations: &[LocationHint {
            platforms: &["macos", "linux"],
            base: "home",
            path: ".claude/projects",
            session_glob: "*/*.jsonl",
            storage_format: "jsonl",
        }],
        capabilities: AdapterCapabilities {
            export_platforms: &["unix"],
            output_formats: &["otlp-jsonl"],
            sdk_caller_owned_cursor: true,
            cursor_source_assumption: "append_only",
            cli_persisted_resume: false,
            delayed_revision_reconciliation: false,
            lossless_archive: false,
        },
    },
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

fn dispatch<const N: usize>(
    registry: &[AdapterRegistration; N],
    args: Vec<OsString>,
    context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    if args.get(1).is_some_and(|arg| arg == "adapters") {
        let descriptors = registry.each_ref().map(|entry| &entry.descriptor);
        return unisphere_cli::run_adapters(args, context, &descriptors, stdout, stderr);
    }
    let name = unisphere_cli::requested_session_adapter(&args);
    match registry.iter().find(|entry| entry.descriptor.id == name) {
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
    dispatch(&ADAPTERS, args, context, stdout, stderr)
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
            descriptor: AdapterDescriptor {
                id: "fixture-text",
                application: "Fixture text",
                description: "Line-based fixture adapter.",
                locations: &[],
                capabilities: ADAPTERS[0].descriptor.capabilities,
            },
            run: |args, context, stdout, stderr| {
                run_with_adapter(TextFixtureAdapter, args, context, stdout, stderr)
            },
        }];
        let context = CliContext {
            cwd: root.path().into(),
            stdout_is_terminal: false,
            version: "test".into(),
        };
        let mut catalog_stdout = Vec::new();
        let mut catalog_stderr = Vec::new();
        assert_eq!(
            dispatch(
                &registry,
                ["unisphere", "adapters", "list", "--json"]
                    .map(OsString::from)
                    .to_vec(),
                &context,
                &mut catalog_stdout,
                &mut catalog_stderr,
            ),
            0
        );
        let catalog: serde_json::Value = serde_json::from_slice(&catalog_stdout).unwrap();
        let ids: Vec<_> = catalog["data"]["adapters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|descriptor| descriptor["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["fixture-text"]);
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

    #[test]
    fn production_catalog_ids_match_exported_provenance() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("session.jsonl"),
            b"{\"type\":\"user\",\"uuid\":\"catalog-proof\",\"message\":{\"role\":\"user\",\"content\":\"synthetic\"}}\n",
        ).unwrap();
        let context = CliContext {
            cwd: root.path().into(),
            stdout_is_terminal: false,
            version: "test".into(),
        };
        for registration in &ADAPTERS {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let args = [
                "unisphere",
                "sessions",
                "export",
                "--adapter",
                registration.descriptor.id,
                "--input",
                "session.jsonl",
            ]
            .map(OsString::from)
            .to_vec();
            assert_eq!(
                (registration.run)(args, &context, &mut stdout, &mut stderr),
                0
            );
            let document: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
            let attributes =
                document["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
                    .as_array()
                    .unwrap();
            let provenance = attributes
                .iter()
                .find(|attribute| attribute["key"] == "unisphere.source.adapter")
                .unwrap();
            assert_eq!(
                provenance["value"]["stringValue"],
                registration.descriptor.id
            );
        }
    }
}
