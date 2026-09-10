use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
};

use serde_json::{Value, json};
use unisphere_cli::{CliContext, run};
use unisphere_core::{
    ConfigOverrides, ConfigSource, Configuration, Failure, InspectionReport, InspectionRequest,
    Location, ReadFailure,
};
use unisphere_testkit::{FakeInspector, fixtures};

fn context(terminal: bool) -> CliContext {
    CliContext {
        cwd: PathBuf::from(if cfg!(windows) {
            r"C:\unisphere-cli-test"
        } else {
            "/unisphere-cli-test"
        }),
        stdout_is_terminal: terminal,
        version: "9.8.7-test".into(),
    }
}

fn report(roots: &[&str]) -> InspectionReport {
    InspectionReport {
        configuration: Configuration {
            source_roots: roots.iter().map(|root| (*root).to_owned()).collect(),
        },
    }
}

struct Invocation {
    code: u8,
    stdout: String,
    stderr: String,
    requests: Vec<InspectionRequest>,
}

fn invoke_os(
    args: Vec<OsString>,
    ctx: &CliContext,
    result: Result<InspectionReport, Failure>,
) -> Invocation {
    let inspector = FakeInspector::new(result);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(args, ctx, &inspector, &mut stdout, &mut stderr);
    Invocation {
        code,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
        requests: inspector.requests(),
    }
}

fn invoke(args: &[&str], terminal: bool, result: Result<InspectionReport, Failure>) -> Invocation {
    invoke_os(
        args.iter().map(OsString::from).collect(),
        &context(terminal),
        result,
    )
}

fn envelope(invocation: &Invocation) -> Value {
    assert!(invocation.stderr.is_empty(), "{}", invocation.stderr);
    assert!(invocation.stdout.ends_with('\n'));
    assert_eq!(
        invocation
            .stdout
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count(),
        1
    );
    assert!(!invocation.stdout.contains('\u{1b}'));
    serde_json::from_str(&invocation.stdout).unwrap()
}

fn assert_failure(invocation: &Invocation, failure: &Failure) {
    let value = envelope(invocation);
    assert_eq!(value["ok"], false);
    assert_eq!(value["command"], "config.check");
    assert_eq!(value["v"], 1);
    assert_eq!(value["error"]["kind"], json!(failure.kind()));
    assert_eq!(value["error"]["code"], failure.code());
    assert_eq!(value["error"]["retryable"], failure.retryable());
    assert_eq!(value["error"]["location"], json!(failure.location()));
    assert!(
        value["next_action"]["summary"]
            .as_str()
            .is_some_and(|summary| !summary.trim().is_empty())
    );
    assert!(
        value["next_action"]["argv"]
            .as_array()
            .is_some_and(|argv| !argv.is_empty())
    );
}

#[test]
fn defaults_call_only_the_injected_inspector_and_render_its_report() {
    let result = report(fixtures::DEFAULT_ROOTS);
    let output = invoke(&["unisphere", "config", "check"], false, Ok(result.clone()));
    assert_eq!(output.code, 0);
    assert_eq!(output.requests, vec![InspectionRequest::default()]);
    let value = envelope(&output);
    assert_eq!(value["data"], json!(result));
    assert!(
        value["next_action"]["summary"]
            .as_str()
            .is_some_and(|summary| !summary.trim().is_empty())
    );
}

#[test]
fn config_paths_are_resolved_lexically_without_discovery_or_canonicalization() {
    let ctx = context(false);
    for supplied in [
        PathBuf::from("missing/../config.json"),
        ctx.cwd.join("absolute.json"),
    ] {
        let expected = if supplied.is_absolute() {
            supplied.clone()
        } else {
            ctx.cwd.join(&supplied)
        };
        let output = invoke_os(
            vec![
                "unisphere".into(),
                "config".into(),
                "check".into(),
                "--config".into(),
                supplied.into_os_string(),
            ],
            &ctx,
            Ok(report(fixtures::DOCUMENT_ROOTS)),
        );
        assert_eq!(output.code, 0);
        assert_eq!(
            output.requests,
            vec![InspectionRequest {
                source: ConfigSource::File(expected),
                overrides: ConfigOverrides::default(),
            }]
        );
        assert_eq!(
            envelope(&output)["data"]["configuration"]["source_roots"],
            json!(fixtures::DOCUMENT_ROOTS)
        );
    }
}

#[test]
fn repeated_roots_and_explicit_clear_have_distinct_requests() {
    let roots = [
        " relative ",
        "~/literal",
        "$HOME/literal",
        "duplicate",
        "duplicate",
        "--json",
    ];
    let mut args: Vec<OsString> = ["unisphere", "config", "check", "--config", "config.json"]
        .map(OsString::from)
        .to_vec();
    for root in roots {
        args.push(format!("--source-root={root}").into());
    }
    let output = invoke_os(args, &context(false), Ok(report(&roots)));
    assert_eq!(output.code, 0);
    assert_eq!(
        output.requests[0].overrides.source_roots,
        Some(roots.map(str::to_owned).to_vec())
    );
    assert_eq!(
        envelope(&output)["data"]["configuration"]["source_roots"],
        json!(roots)
    );

    let cleared = invoke(
        &[
            "unisphere",
            "config",
            "check",
            "--config",
            "config.json",
            "--clear-source-roots",
        ],
        false,
        Ok(report(&[])),
    );
    assert_eq!(cleared.code, 0);
    assert_eq!(cleared.requests[0].overrides.source_roots, Some(vec![]));
    assert_eq!(
        envelope(&cleared)["data"]["configuration"]["source_roots"],
        json!([])
    );

    let replaced = invoke(
        &[
            "unisphere",
            "config",
            "check",
            "--source-root",
            fixtures::OVERRIDE_ROOTS[0],
        ],
        false,
        Ok(report(fixtures::OVERRIDE_ROOTS)),
    );
    assert_eq!(replaced.requests[0].source, ConfigSource::Defaults);
    assert_eq!(
        replaced.requests[0].overrides.source_roots,
        Some(vec![fixtures::OVERRIDE_ROOTS[0].to_owned()])
    );
}

#[test]
fn blank_root_is_configuration_input_not_a_parser_diagnostic() {
    let failure = Failure::invalid_configuration(Some(Location {
        field: Some("source_roots[0]".into()),
        ..Location::default()
    }));
    for root in ["", " \t\n"] {
        let output = invoke(
            &["unisphere", "config", "check", "--source-root", root],
            false,
            Err(failure.clone()),
        );
        assert_eq!(output.code, 1);
        assert_eq!(
            output.requests[0].overrides.source_roots,
            Some(vec![root.to_owned()])
        );
        assert_failure(&output, &failure);
    }
}

#[test]
fn output_modes_override_terminal_status_for_success_and_failure() {
    for terminal in [false, true] {
        for flag in [None, Some("--json"), Some("--human")] {
            let mut args = vec!["unisphere", "config", "check"];
            if let Some(flag) = flag {
                args.push(flag);
            }
            let is_json = flag == Some("--json") || (flag.is_none() && !terminal);
            let success = invoke(&args, terminal, Ok(report(fixtures::DOCUMENT_ROOTS)));
            assert_eq!(success.code, 0);
            if is_json {
                assert_eq!(
                    envelope(&success)["data"]["configuration"]["source_roots"],
                    json!(fixtures::DOCUMENT_ROOTS)
                );
            } else {
                assert!(success.stderr.is_empty());
                assert!(success.stdout.contains("Configuration valid."));
                for root in fixtures::DOCUMENT_ROOTS {
                    assert!(
                        success
                            .stdout
                            .contains(&serde_json::to_string(root).unwrap())
                    );
                }
            }
            let failure = Failure::configuration_read(ReadFailure::NotFound, None);
            let failed = invoke(&args, terminal, Err(failure.clone()));
            assert_eq!(failed.code, 1);
            if is_json {
                assert_failure(&failed, &failure);
            } else {
                assert!(failed.stdout.is_empty());
                assert!(failed.stderr.contains(failure.code()));
                assert!(failed.stderr.contains(failure.message()));
                assert!(failed.stderr.contains(failure.fix()));
            }
        }
    }
}

#[test]
fn global_modes_work_before_between_and_after_subcommands() {
    for args in [
        vec!["unisphere", "--json", "config", "check"],
        vec!["unisphere", "config", "--json", "check"],
        vec!["unisphere", "config", "check", "--json"],
    ] {
        let output = invoke(&args, true, Ok(report(&[])));
        assert_eq!(output.code, 0);
        assert_eq!(envelope(&output)["ok"], true);
    }
}

#[test]
fn conflicting_modes_always_fail_in_json_regardless_of_order_or_terminal() {
    for terminal in [false, true] {
        for flags in [["--json", "--human"], ["--human", "--json"]] {
            let output = invoke(
                &["unisphere", "config", "check", flags[0], flags[1]],
                terminal,
                Ok(report(&[])),
            );
            assert_eq!(output.code, 2);
            assert!(output.requests.is_empty());
            assert_failure(&output, &Failure::invalid_arguments(None));
        }
    }
}

#[test]
fn invalid_invocations_are_safe_typed_failures_without_port_calls() {
    for args in [
        vec![],
        vec!["unisphere"],
        vec!["unisphere", "config"],
        vec!["unisphere", "SENSITIVE-ARG-MARKER"],
        vec!["unisphere", "config", "SENSITIVE-ARG-MARKER"],
        vec!["unisphere", "config", "check", "--config"],
        vec!["unisphere", "config", "check", "--source-root"],
        vec!["unisphere", "config", "check", "SENSITIVE-ARG-MARKER"],
        vec!["unisphere", "config", "check", "--SENSITIVE-ARG-MARKER"],
        vec![
            "unisphere",
            "config",
            "check",
            "--json=SENSITIVE-ARG-MARKER",
        ],
        vec![
            "unisphere",
            "config",
            "check",
            "--source-root",
            "SENSITIVE-ARG-MARKER",
            "--clear-source-roots",
        ],
        vec![
            "unisphere",
            "config",
            "check",
            "--clear-source-roots",
            "--source-root",
            "SENSITIVE-ARG-MARKER",
        ],
    ] {
        let output = invoke(&args, false, Ok(report(&[])));
        assert_eq!(output.code, 2, "args: {args:?}");
        assert!(output.requests.is_empty());
        assert_failure(&output, &Failure::invalid_arguments(None));
        assert!(!output.stdout.contains("SENSITIVE-ARG-MARKER"));
    }
    let human = invoke(
        &[
            "unisphere",
            "config",
            "check",
            "--human",
            "--SENSITIVE-ARG-MARKER",
        ],
        false,
        Ok(report(&[])),
    );
    assert_eq!(human.code, 2);
    assert!(human.stdout.is_empty());
    assert!(human.stderr.contains("UNI-ARGS-INVALID"));
    assert!(!human.stderr.contains("SENSITIVE-ARG-MARKER"));
}

#[test]
fn mode_looking_values_and_end_of_options_do_not_select_a_mode() {
    for value in ["--source-root=--json", "--config=--json"] {
        let output = invoke(
            &["unisphere", "config", "check", value],
            true,
            Ok(report(&[])),
        );
        assert_eq!(output.code, 0);
        assert!(output.stdout.starts_with("Configuration valid."));
    }
    let output = invoke(
        &["unisphere", "config", "check", "--", "--json"],
        true,
        Ok(report(&[])),
    );
    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.contains("UNI-ARGS-INVALID"));
}

#[test]
fn help_at_every_command_level_honors_output_mode_without_inspection() {
    for command in [
        vec!["unisphere"],
        vec!["unisphere", "config"],
        vec!["unisphere", "config", "check"],
    ] {
        for terminal in [false, true] {
            for flag in [None, Some("--json"), Some("--human")] {
                let mut args = command.clone();
                args.push("--help");
                if let Some(flag) = flag {
                    args.push(flag);
                }
                let output = invoke(&args, terminal, Err(Failure::invalid_configuration(None)));
                assert_eq!(output.code, 0);
                assert!(output.requests.is_empty());
                assert!(output.stderr.is_empty());
                if flag == Some("--json") || (flag.is_none() && !terminal) {
                    let value = envelope(&output);
                    assert_eq!(value["command"], "help");
                    assert_eq!(value["ok"], true);
                    assert_eq!(value["v"], 1);
                    assert!(value.get("error").is_none());
                    assert!(value["data"]["text"].as_str().unwrap().contains("Usage:"));
                } else {
                    assert!(output.stdout.contains("Usage:"));
                    assert!(!output.stdout.contains('\u{1b}'));
                }
            }
        }
    }
    let short = invoke(
        &["unisphere", "config", "check", "-h"],
        false,
        Ok(report(&[])),
    );
    assert_eq!(short.code, 0);
    assert!(
        envelope(&short)["data"]["text"]
            .as_str()
            .unwrap()
            .contains("--clear-source-roots")
    );
}

#[test]
fn version_uses_explicit_context_and_selected_mode_without_inspection() {
    for terminal in [false, true] {
        for flag in [None, Some("--json"), Some("--human")] {
            for version_flag in ["--version", "-V"] {
                let mut args = vec!["unisphere", version_flag];
                if let Some(flag) = flag {
                    args.push(flag);
                }
                let output = invoke(&args, terminal, Err(Failure::invalid_configuration(None)));
                assert_eq!(output.code, 0);
                assert!(output.requests.is_empty());
                assert!(output.stderr.is_empty());
                if flag == Some("--json") || (flag.is_none() && !terminal) {
                    let value = envelope(&output);
                    assert_eq!(value["data"]["version"], "9.8.7-test");
                    assert!(
                        value["next_action"]["summary"]
                            .as_str()
                            .is_some_and(|summary| !summary.trim().is_empty())
                    );
                } else {
                    assert!(output.stdout.starts_with("unisphere 9.8.7-test\n"));
                    assert!(output.stdout.contains("Next:"));
                }
            }
        }
    }
}

#[test]
fn failures_preserve_core_semantics_and_structural_location() {
    let location = Some(Location {
        path: Some(
            context(false)
                .cwd
                .join("config.json")
                .to_string_lossy()
                .into_owned(),
        ),
        field: Some("source_roots[2]".into()),
        line: Some(4),
        column: Some(9),
    });
    let failures = [
        Failure::invalid_configuration(location.clone()),
        Failure::invalid_arguments(location.clone()),
        Failure::configuration_read(ReadFailure::TooLarge, location.clone()),
        Failure::configuration_read(ReadFailure::Other, location.clone()),
    ];
    let shared_failures = fixtures::READ_FAILURES.iter().map(|(kind, expected_kind)| {
        let failure = Failure::configuration_read(*kind, location.clone());
        assert_eq!(failure.kind(), *expected_kind);
        failure
    });
    for failure in failures.into_iter().chain(shared_failures) {
        let output = invoke(
            &["unisphere", "config", "check", "--config", "config.json"],
            false,
            Err(failure.clone()),
        );
        let expected_code = if failure.code() == "UNI-ARGS-INVALID" {
            2
        } else {
            1
        };
        assert_eq!(output.code, expected_code);
        assert_eq!(output.requests.len(), 1);
        assert_failure(&output, &failure);
    }
    let output = invoke(
        &["unisphere", "config", "check", "--human"],
        false,
        Err(Failure::invalid_configuration(location)),
    );
    assert_eq!(output.code, 1);
    assert!(output.stdout.is_empty());
    for fragment in [
        "Path:",
        "Field: \"source_roots[2]\"",
        "Line: 4",
        "Column: 9",
        "Fix:",
    ] {
        assert!(output.stderr.contains(fragment));
    }
}

#[test]
fn request_values_never_leak_into_failure_diagnostics() {
    let failure = Failure::invalid_configuration(None);
    for mode in ["--json", "--human"] {
        let output = invoke(
            &[
                "unisphere",
                "config",
                "check",
                "--source-root",
                "SENSITIVE-CONFIG-MARKER",
                mode,
            ],
            false,
            Err(failure.clone()),
        );
        assert_eq!(output.code, 1);
        assert_eq!(
            output.requests[0].overrides.source_roots,
            Some(vec!["SENSITIVE-CONFIG-MARKER".into()])
        );
        assert!(!output.stdout.contains("SENSITIVE-CONFIG-MARKER"));
        assert!(!output.stderr.contains("SENSITIVE-CONFIG-MARKER"));
    }
}

#[test]
fn output_escapes_control_characters_without_changing_semantic_data() {
    let roots = ["quote\"slash\\newline\n\u{1b}[31m", "snowman \u{2603}"];
    let machine = invoke(&["unisphere", "config", "check"], false, Ok(report(&roots)));
    assert_eq!(
        envelope(&machine)["data"]["configuration"]["source_roots"],
        json!(roots)
    );
    let human = invoke(
        &["unisphere", "config", "check", "--human"],
        false,
        Ok(report(&roots)),
    );
    assert!(!human.stdout.contains('\u{1b}'));
    assert!(
        human
            .stdout
            .contains(&serde_json::to_string(roots[0]).unwrap())
    );
    let failure = Failure::configuration_read(
        ReadFailure::NotFound,
        Some(Location {
            path: Some("config\n\u{1b}[31m.json".into()),
            ..Location::default()
        }),
    );
    let human_error = invoke(
        &["unisphere", "config", "check", "--human"],
        false,
        Err(failure),
    );
    assert!(!human_error.stderr.contains('\u{1b}'));
    assert!(human_error.stderr.contains("config\\n\\u001b[31m.json"));
}

#[test]
fn a_relative_context_cannot_silently_create_a_relative_file_request() {
    let mut ctx = context(false);
    ctx.cwd = PathBuf::from("relative-context");
    let output = invoke_os(
        ["unisphere", "config", "check", "--config", "config.json"]
            .map(OsString::from)
            .to_vec(),
        &ctx,
        Ok(report(&[])),
    );
    assert_eq!(output.code, 2);
    assert!(output.requests.is_empty());
    assert_failure(&output, &Failure::invalid_arguments(None));
}

#[cfg(unix)]
#[test]
fn non_utf8_file_paths_are_preserved_but_non_utf8_root_values_are_rejected_safely() {
    use std::os::unix::ffi::OsStringExt;
    let hostile = OsString::from_vec(b"SENSITIVE-ARG-MARKER\xff".to_vec());
    let ctx = context(false);
    let file = invoke_os(
        vec![
            "unisphere".into(),
            "config".into(),
            "check".into(),
            "--config".into(),
            hostile.clone(),
        ],
        &ctx,
        Ok(report(&[])),
    );
    assert_eq!(file.code, 0);
    assert_eq!(
        file.requests[0].source,
        ConfigSource::File(ctx.cwd.join(&hostile))
    );
    let root = invoke_os(
        vec![
            "unisphere".into(),
            "config".into(),
            "check".into(),
            "--source-root".into(),
            hostile,
        ],
        &ctx,
        Ok(report(&[])),
    );
    assert_eq!(root.code, 2);
    assert!(root.requests.is_empty());
    assert_failure(&root, &Failure::invalid_arguments(None));
    assert!(!root.stdout.contains("SENSITIVE-ARG-MARKER"));
}

struct FaultWriter {
    bytes: Vec<u8>,
    allowance: usize,
    fail_flush: bool,
}

impl Write for FaultWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.allowance == 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "SENSITIVE-WRITER-MARKER",
            ));
        }
        let size = bytes.len().min(self.allowance).min(3);
        self.bytes.extend_from_slice(&bytes[..size]);
        self.allowance -= size;
        Ok(size)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::Error::other("SENSITIVE-WRITER-MARKER"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn short_writes_are_completed_and_write_or_flush_failures_exit_one_safely() {
    for args in [
        vec!["unisphere", "config", "check"],
        vec!["unisphere", "--help"],
        vec!["unisphere", "--version"],
        vec!["unisphere", "config", "check", "--human"],
    ] {
        for (allowance, fail_flush, expected) in [
            (usize::MAX, false, 0),
            (0, false, 1),
            (12, false, 1),
            (usize::MAX, true, 1),
        ] {
            let mut stdout = FaultWriter {
                bytes: vec![],
                allowance,
                fail_flush,
            };
            let mut stderr = Vec::new();
            let inspector = FakeInspector::new(Ok(report(&[])));
            let code = run(
                args.iter().map(OsString::from),
                &context(false),
                &inspector,
                &mut stdout,
                &mut stderr,
            );
            assert_eq!(code, expected);
            if expected == 0 {
                assert!(stderr.is_empty());
                assert!(stdout.bytes.ends_with(b"\n"));
            } else {
                assert!(String::from_utf8_lossy(&stderr).contains("output incomplete"));
                assert!(
                    !String::from_utf8_lossy(&stdout.bytes).contains("SENSITIVE-WRITER-MARKER")
                );
            }
        }
    }
}

#[test]
fn failure_output_errors_override_argument_exit_and_tolerate_broken_stderr() {
    for args in [
        vec!["unisphere", "--invalid"],
        vec!["unisphere", "--invalid", "--human"],
        vec!["unisphere", "config", "check"],
        vec!["unisphere", "config", "check", "--human"],
    ] {
        let inspector = FakeInspector::new(Err(Failure::configuration_read(
            ReadFailure::PermissionDenied,
            None,
        )));
        let mut stdout = FaultWriter {
            bytes: vec![],
            allowance: 0,
            fail_flush: false,
        };
        let mut stderr = FaultWriter {
            bytes: vec![],
            allowance: 0,
            fail_flush: false,
        };
        assert_eq!(
            run(
                args.iter().map(OsString::from),
                &context(false),
                &inspector,
                &mut stdout,
                &mut stderr
            ),
            1
        );
    }
    let inspector = FakeInspector::new(Err(Failure::invalid_arguments(None)));
    let mut stdout = Vec::new();
    let mut stderr = FaultWriter {
        bytes: vec![],
        allowance: usize::MAX,
        fail_flush: true,
    };
    assert_eq!(
        run(
            ["unisphere", "config", "check", "--human"].map(OsString::from),
            &context(false),
            &inspector,
            &mut stdout,
            &mut stderr
        ),
        1
    );
    assert!(stdout.is_empty());
    assert!(!String::from_utf8_lossy(&stderr.bytes).contains("SENSITIVE-WRITER-MARKER"));
}
