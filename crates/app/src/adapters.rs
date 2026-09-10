//! Explicit compile-time adapter registration; no plugin loader or service locator.
#[cfg(test)]
use std::ffi::OsString;
use std::{io::Write, sync::Arc};
use unisphere_cli::{
    AdapterCapabilities, AdapterDescriptor, CliContext, LocationHint, ParsedCommand,
};
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_loader_query::{NativeRepresentation, QueryRegistration};
use unisphere_loader_snapshot::FileSnapshotLoader;
use unisphere_output_otlp::OtlpJsonlWriter;
use unisphere_sdk::query::{HarnessId, QueryAdapter, QueryFailure};
use unisphere_sdk::{
    Collector, PipelineError, PipelineErrorKind, SessionAdapter, SnapshotAdapter,
    SnapshotCollector, SnapshotFormat,
};

#[derive(Clone, Copy)]
enum SourceRepresentation {
    Jsonl,
    JsonDocument,
    SqliteKeyValue(&'static str),
}

type QueryFactory = fn(&AdapterRegistration) -> Result<QueryRegistration, QueryFailure>;

struct AdapterRegistration {
    descriptor: AdapterDescriptor,
    source: SourceRepresentation,
    query: Option<QueryFactory>,
    run:
        fn(SourceRepresentation, &ParsedCommand, &CliContext, &mut dyn Write, &mut dyn Write) -> u8,
    #[cfg(test)]
    fixture: &'static [u8],
}

const ADAPTERS: [AdapterRegistration; 9] = [
    AdapterRegistration {
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
        source: SourceRepresentation::Jsonl,
        query: Some(|entry| {
            query_registration(
                entry,
                "claude-code",
                unisphere_adapter_claude::QUERY_POLICY_VERSION,
                unisphere_adapter_claude::ClaudeCodeAdapter,
            )
        }),
        run: |_, args, context, stdout, stderr| {
            run_with_adapter(
                unisphere_adapter_claude::ClaudeCodeAdapter,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: unisphere_testkit::collection::CLAUDE_BASIC,
    },
    AdapterRegistration {
        descriptor: unisphere_adapter_codex::DESCRIPTOR,
        source: SourceRepresentation::Jsonl,
        query: Some(|entry| {
            query_registration(
                entry,
                "codex",
                unisphere_adapter_codex::QUERY_POLICY_VERSION,
                unisphere_adapter_codex::CodexAdapter,
            )
        }),
        run: |_, args, context, stdout, stderr| {
            run_with_adapter(
                unisphere_adapter_codex::CodexAdapter,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-codex/fixtures/rollout.jsonl"),
    },
    AdapterRegistration {
        descriptor: unisphere_adapter_omp::DESCRIPTOR,
        source: SourceRepresentation::Jsonl,
        query: Some(|entry| {
            query_registration(
                entry,
                "oh-my-pi",
                unisphere_adapter_omp::POLICY_VERSION,
                unisphere_adapter_omp::OmpAdapter,
            )
        }),
        run: |_, args, context, stdout, stderr| {
            run_with_adapter(
                unisphere_adapter_omp::OmpAdapter,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-omp/tests/fixtures/native.jsonl"),
    },
    AdapterRegistration {
        descriptor: unisphere_adapter_pi::DESCRIPTOR,
        source: SourceRepresentation::Jsonl,
        query: Some(|entry| {
            query_registration(
                entry,
                "pi",
                unisphere_adapter_pi::POLICY_VERSION,
                unisphere_adapter_pi::PiAdapter,
            )
        }),
        run: |_, args, context, stdout, stderr| {
            run_with_adapter(
                unisphere_adapter_pi::PiAdapter,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-pi/tests/fixtures/v3-tree.jsonl"),
    },
    AdapterRegistration {
        descriptor: unisphere_adapter_copilot_cli::DESCRIPTOR,
        source: SourceRepresentation::Jsonl,
        query: Some(|entry| {
            query_registration(
                entry,
                "copilot-cli",
                unisphere_adapter_copilot_cli::CURRENT_QUERY_POLICY_VERSION,
                unisphere_adapter_copilot_cli::CopilotCliAdapter,
            )
        }),
        run: |_, args, context, stdout, stderr| {
            run_with_adapter(
                unisphere_adapter_copilot_cli::CopilotCliAdapter,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-copilot-cli/tests/fixtures/events.jsonl"),
    },
    AdapterRegistration {
        descriptor: unisphere_adapter_cursor::DESCRIPTOR,
        source: SourceRepresentation::Jsonl,
        query: Some(|entry| {
            query_registration(
                entry,
                "cursor",
                unisphere_adapter_cursor::TRANSCRIPT_POLICY,
                unisphere_adapter_cursor::CursorAdapter,
            )
        }),
        run: |_, args, context, stdout, stderr| {
            run_with_adapter(
                unisphere_adapter_cursor::CursorAdapter,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-cursor/fixtures/transcript.jsonl"),
    },
    AdapterRegistration {
        descriptor: unisphere_adapter_vscode_copilot::DESCRIPTOR,
        source: SourceRepresentation::JsonDocument,
        query: Some(|entry| {
            query_registration(
                entry,
                "copilot",
                unisphere_adapter_vscode_copilot::QUERY_POLICY_VERSION,
                unisphere_adapter_vscode_copilot::VsCodeCopilotAdapter,
            )
        }),
        run: |source, args, context, stdout, stderr| {
            run_with_snapshot(
                unisphere_adapter_vscode_copilot::VsCodeCopilotAdapter,
                source,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-vscode-copilot/tests/fixtures/session-v3.json"),
    },
    AdapterRegistration {
        descriptor: unisphere_adapter_cursor::IDE_DESCRIPTOR,
        source: SourceRepresentation::SqliteKeyValue("cursorDiskKV"),
        query: Some(|entry| {
            query_registration(
                entry,
                "cursor",
                unisphere_adapter_cursor::IDE_POLICY,
                unisphere_adapter_cursor::CursorIdeAdapter,
            )
        }),
        run: |source, args, context, stdout, stderr| {
            run_with_snapshot(
                unisphere_adapter_cursor::CursorIdeAdapter,
                source,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-cursor/fixtures/ide.json"),
    },
    AdapterRegistration {
        descriptor: unisphere_adapter_copilot_cli::SNAPSHOT_DESCRIPTOR,
        source: SourceRepresentation::JsonDocument,
        query: Some(|entry| {
            query_registration(
                entry,
                "copilot-cli",
                unisphere_adapter_copilot_cli::LEGACY_QUERY_POLICY_VERSION,
                unisphere_adapter_copilot_cli::CopilotCliAdapterSnapshot,
            )
        }),
        run: |source, args, context, stdout, stderr| {
            run_with_snapshot(
                unisphere_adapter_copilot_cli::CopilotCliAdapterSnapshot,
                source,
                args,
                context,
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-copilot-cli/tests/fixtures/legacy.json"),
    },
];

fn query_registration<A: QueryAdapter + 'static>(
    entry: &AdapterRegistration,
    harness: &str,
    policy: &str,
    adapter: A,
) -> Result<QueryRegistration, QueryFailure> {
    let representation = match entry.source {
        SourceRepresentation::Jsonl => NativeRepresentation::Jsonl,
        SourceRepresentation::JsonDocument => NativeRepresentation::JsonDocument,
        SourceRepresentation::SqliteKeyValue(table) => NativeRepresentation::SqliteKeyValue {
            table: table.into(),
        },
    };
    Ok(QueryRegistration::new(
        entry.descriptor,
        HarnessId::new(harness).map_err(|_| QueryFailure::invalid_data())?,
        representation,
        Arc::new(adapter),
    )
    .with_query_policy_version(policy))
}

pub fn query_registrations() -> Result<Vec<QueryRegistration>, QueryFailure> {
    let mut registrations = Vec::with_capacity(ADAPTERS.len() + 1);
    for entry in &ADAPTERS {
        let Some(make_query) = entry.query else {
            continue;
        };
        let registration = make_query(entry)?;
        if matches!(entry.source, SourceRepresentation::JsonDocument)
            && entry
                .descriptor
                .locations
                .iter()
                .any(|hint| hint.storage_format == "json_journal")
        {
            let mut journal = registration.clone();
            journal.representation = NativeRepresentation::JsonJournal;
            registrations.push(journal);
        }
        registrations.push(registration);
    }
    Ok(registrations)
}

fn run_with_adapter<A: SessionAdapter>(
    adapter: A,
    command: &ParsedCommand,
    _context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let collector = Collector::new(FileSessionLoader, adapter, OtlpJsonlWriter);
    match command {
        ParsedCommand::NativeRootList(command) => {
            unisphere_cli::run_native_list(command, &collector, stdout, stderr)
        }
        ParsedCommand::NativeExport(command) => {
            unisphere_cli::run_native_export(command, &collector, stdout, stderr)
        }
        _ => invalid_native(stderr),
    }
}

fn run_with_snapshot<A: SnapshotAdapter>(
    adapter: A,
    source: SourceRepresentation,
    command: &ParsedCommand,
    _context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let format = match source {
        SourceRepresentation::JsonDocument => SnapshotFormat::JsonDocument,
        SourceRepresentation::SqliteKeyValue(table) => SnapshotFormat::SqliteKeyValue {
            table: table.into(),
        },
        SourceRepresentation::Jsonl => {
            return unisphere_cli::session_error(
                stderr,
                &PipelineError::new(PipelineErrorKind::InvalidInput, None),
                2,
            );
        }
    };
    let collector = SnapshotCollector::new(FileSnapshotLoader, adapter, OtlpJsonlWriter);
    match command {
        ParsedCommand::NativeExport(command) => {
            unisphere_cli::run_native_snapshot_export(command, &collector, format, stdout, stderr)
        }
        _ => invalid_native(stderr),
    }
}

fn dispatch<const N: usize>(
    registry: &[AdapterRegistration; N],
    command: &ParsedCommand,
    context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let name = match command {
        ParsedCommand::Catalog(command) => {
            let descriptors = registry.each_ref().map(|entry| &entry.descriptor);
            return unisphere_cli::run_catalog(command, &descriptors, stdout, stderr);
        }
        ParsedCommand::NativeRootList(_) => {
            return run_with_adapter(
                unisphere_adapter_claude::ClaudeCodeAdapter,
                command,
                context,
                stdout,
                stderr,
            );
        }
        ParsedCommand::NativeExport(command) => &command.adapter,
        ParsedCommand::NativeGitNotesList(command) => &command.adapter,
        _ => return invalid_native(stderr),
    };
    match registry.iter().find(|entry| entry.descriptor.id == name) {
        Some(entry) => (entry.run)(entry.source, command, context, stdout, stderr),
        None => invalid_native(stderr),
    }
}

fn invalid_native(stderr: &mut dyn Write) -> u8 {
    unisphere_cli::session_error(
        stderr,
        &PipelineError::new(PipelineErrorKind::InvalidInput, None),
        2,
    )
}

pub fn run(
    command: &ParsedCommand,
    context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    dispatch(&ADAPTERS, command, context, stdout, stderr)
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
            source: SourceRepresentation::Jsonl,
            query: None,
            run: |_, args, context, stdout, stderr| {
                run_with_adapter(TextFixtureAdapter, args, context, stdout, stderr)
            },
            fixture: b"SENSITIVE-CUSTOM-TEXT\n",
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
                &unisphere_cli::parse(
                    ["unisphere", "adapters", "list", "--json"]
                        .map(OsString::from)
                        .to_vec(),
                    &context
                )
                .unwrap(),
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
            dispatch(
                &registry,
                &unisphere_cli::parse(args, &context).unwrap(),
                &context,
                &mut stdout,
                &mut stderr
            ),
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
        let context = CliContext {
            cwd: root.path().into(),
            stdout_is_terminal: false,
            version: "test".into(),
        };
        for registration in &ADAPTERS {
            let input = root
                .path()
                .join(format!("{}.fixture", registration.descriptor.id));
            match registration.source {
                SourceRepresentation::Jsonl | SourceRepresentation::JsonDocument => {
                    std::fs::write(&input, registration.fixture).unwrap();
                }
                SourceRepresentation::SqliteKeyValue(table) => {
                    let db = rusqlite::Connection::open(&input).unwrap();
                    db.execute_batch(&format!(
                        "CREATE TABLE \"{table}\" (key TEXT PRIMARY KEY, value BLOB)"
                    ))
                    .unwrap();
                    let rows: serde_json::Value =
                        serde_json::from_slice(registration.fixture).unwrap();
                    for row in rows.as_array().unwrap() {
                        db.execute(
                            &format!("INSERT INTO \"{table}\" VALUES (?1, ?2)"),
                            rusqlite::params![
                                row["key"].as_str().unwrap(),
                                serde_json::to_vec(&row["value"]).unwrap()
                            ],
                        )
                        .unwrap();
                    }
                }
            }
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let args = [
                "unisphere",
                "sessions",
                "export",
                "--adapter",
                registration.descriptor.id,
                "--input",
                input.to_str().unwrap(),
                "--include-content",
            ]
            .map(OsString::from)
            .to_vec();
            assert_eq!(
                (registration.run)(
                    registration.source,
                    &unisphere_cli::parse(args, &context).unwrap(),
                    &context,
                    &mut stdout,
                    &mut stderr
                ),
                0
            );
            assert!(String::from_utf8_lossy(&stdout).contains("SENSITIVE"));
            for line in std::str::from_utf8(&stdout).unwrap().lines() {
                let document: serde_json::Value = serde_json::from_str(line).unwrap();
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
                let has = |key: &str| attributes.iter().any(|attribute| attribute["key"] == key);
                match registration.source {
                    SourceRepresentation::Jsonl => assert!(has("unisphere.source.offset")),
                    _ => {
                        assert!(!has("unisphere.source.offset"));
                        assert!(
                            has("unisphere.source.key")
                                && has("unisphere.source.revision")
                                && has("unisphere.source.format")
                        );
                    }
                }
            }
        }
    }
}
