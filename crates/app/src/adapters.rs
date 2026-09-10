//! Explicit compile-time adapter registration; no plugin loader or service locator.
use std::{ffi::OsString, io::Write};
use unisphere_cli::{AdapterCapabilities, AdapterDescriptor, CliContext, LocationHint};
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_loader_snapshot::FileSnapshotLoader;
use unisphere_output_otlp::OtlpJsonlWriter;
use unisphere_sdk::{
    Collector, PipelineError, PipelineErrorKind, SessionAdapter, SnapshotAdapter,
    SnapshotCollector, SnapshotFormat,
};

#[derive(Clone, Copy)]
enum SourceRepresentation {
    Jsonl,
    JsonDocument,
    SqliteKeyValue(&'static str),
    GitNotes,
}

struct AdapterRegistration {
    descriptor: AdapterDescriptor,
    source: SourceRepresentation,
    run: fn(SourceRepresentation, Vec<OsString>, &CliContext, &mut dyn Write, &mut dyn Write) -> u8,
    #[cfg(test)]
    fixture: &'static [u8],
}

const ADAPTERS: [AdapterRegistration; 10] = [
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
    AdapterRegistration {
        descriptor: unisphere_adapter_git_ai::DESCRIPTOR,
        source: SourceRepresentation::GitNotes,
        run: |_, args, context, stdout, stderr| {
            unisphere_cli::run_git_notes(
                args,
                context,
                |explicit| {
                    let git = resolve_git(explicit, &context.cwd)?;
                    Ok(unisphere_sdk::GitNotesCollector::new(
                        unisphere_loader_git::GitObjectLoader::new(git),
                        unisphere_adapter_git_ai::GitAiAdapter,
                        OtlpJsonlWriter,
                    ))
                },
                stdout,
                stderr,
            )
        },
        #[cfg(test)]
        fixture: include_bytes!("../../adapter-git-ai/fixtures/mixed.notes"),
    },
];

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

fn resolve_git(
    explicit: Option<std::path::PathBuf>,
    cwd: &std::path::Path,
) -> Result<std::path::PathBuf, unisphere_sdk::GitNotesError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let usable = |path: std::path::PathBuf| {
            let path = std::fs::canonicalize(path).ok()?;
            let metadata = path.metadata().ok()?;
            (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(path)
        };
        if let Some(path) = explicit {
            return usable(path).ok_or(unisphere_sdk::GitNotesError::GitUnavailable);
        }
        let path = std::env::var_os("PATH").ok_or(unisphere_sdk::GitNotesError::GitUnavailable)?;
        if path.is_empty() {
            return Err(unisphere_sdk::GitNotesError::GitUnavailable);
        }
        std::env::split_paths(&path)
            .map(|directory| {
                if directory.is_absolute() {
                    directory.join("git")
                } else {
                    cwd.join(directory).join("git")
                }
            })
            .find_map(usable)
            .ok_or(unisphere_sdk::GitNotesError::GitUnavailable)
    }
    #[cfg(not(unix))]
    {
        let _ = (explicit, cwd);
        Err(unisphere_sdk::GitNotesError::UnsupportedPlatform)
    }
}

fn run_with_snapshot<A: SnapshotAdapter>(
    adapter: A,
    source: SourceRepresentation,
    args: Vec<OsString>,
    context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let format = match source {
        SourceRepresentation::JsonDocument => SnapshotFormat::JsonDocument,
        SourceRepresentation::SqliteKeyValue(table) => SnapshotFormat::SqliteKeyValue {
            table: table.into(),
        },
        SourceRepresentation::Jsonl | SourceRepresentation::GitNotes => {
            return unisphere_cli::session_error(
                stderr,
                &PipelineError::new(PipelineErrorKind::InvalidInput, None),
                2,
            );
        }
    };
    let collector = SnapshotCollector::new(FileSnapshotLoader, adapter, OtlpJsonlWriter);
    unisphere_cli::run_snapshot_sessions(args, context, &collector, format, stdout, stderr)
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
        Some(entry) => (entry.run)(entry.source, args, context, stdout, stderr),
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
            source: SourceRepresentation::Jsonl,
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
                SourceRepresentation::GitNotes => {
                    let git = unisphere_testkit::git_notes::standard_git().unwrap();
                    unisphere_testkit::git_notes::initialize(
                        &input,
                        &git,
                        false,
                        registration.fixture,
                    )
                    .unwrap();
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
            let mut args = [
                "unisphere",
                "sessions",
                "export",
                "--adapter",
                registration.descriptor.id,
                if matches!(registration.source, SourceRepresentation::GitNotes) {
                    "--repo"
                } else {
                    "--input"
                },
                input.to_str().unwrap(),
                "--include-content",
            ]
            .map(OsString::from)
            .to_vec();
            if matches!(registration.source, SourceRepresentation::GitNotes) {
                args.push("--git-executable".into());
                args.push(
                    unisphere_testkit::git_notes::standard_git()
                        .unwrap()
                        .into_os_string(),
                );
            }
            assert_eq!(
                (registration.run)(
                    registration.source,
                    args,
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
