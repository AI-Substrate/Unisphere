#![cfg(unix)]

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::{TempDir, tempdir};
use unisphere_loader_query::pij::{
    PijLookupError, PijLookupLimits, ResolvedPijSession, resolve_pij,
};

const ID: &str = "pij-test-seat";
const SCRIPT: &str = "#!/bin/sh\nif [ \"$1\" != state ] || [ \"$2\" != pij-test-seat ] || [ \"$3\" != --json ] || [ \"$#\" != 3 ]; then exit 90; fi\nIFS= read -r response < \"$0.response\"\nprintf '%s' \"$response\"\nIFS= read -r status < \"$0.status\"\nexit \"$status\"\n";

fn sidecar(executable: &Path, suffix: &str) -> PathBuf {
    let mut path = OsString::from(executable.as_os_str());
    path.push(suffix);
    PathBuf::from(path)
}

fn program(response: &str, status: u8) -> (TempDir, PathBuf) {
    let temporary = tempdir().unwrap();
    let executable = temporary.path().join("pij-fixture");
    fs::write(&executable, SCRIPT).unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&executable, permissions).unwrap();
    set_response(&executable, response);
    fs::write(sidecar(&executable, ".status"), status.to_string()).unwrap();
    (temporary, executable)
}

fn set_response(executable: &Path, response: &str) {
    fs::write(sidecar(executable, ".response"), response).unwrap();
}

fn resolve(executable: &Path) -> Result<ResolvedPijSession, PijLookupError> {
    resolve_pij(Some(executable), ID, PijLookupLimits::default())
}

#[test]
fn live_and_tombstoned_seats_resolve_only_typed_identity_and_hints() {
    let (_temporary, executable) = program(
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":"omp","session":"native-1","cwd":"/synthetic/project","machine":"fixture-host","pid":77,"pane":"%9","tombstonedAt":null}}"#,
        0,
    );
    let live = resolve(&executable).unwrap();
    assert_eq!(live.pij_id, ID);
    assert_eq!(live.harness.as_str(), "oh-my-pi");
    assert_eq!(live.native_session_id, "native-1");
    assert_eq!(live.cwd.as_deref(), Some(Path::new("/synthetic/project")));
    assert_eq!(live.machine.as_deref(), Some("fixture-host"));

    set_response(
        &executable,
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":"omp","session":"native-1","cwd":null,"machine":null,"pid":null,"pane":null,"tombstonedAt":1789000000000}}"#,
    );
    let retired = resolve(&executable).unwrap();
    assert_eq!(retired.native_session_id, "native-1");
    assert_eq!(retired.cwd, None);
    assert_eq!(retired.machine, None);
}

#[test]
fn every_call_reads_current_mapping_and_translates_supported_harnesses() {
    let (_temporary, executable) = program("", 0);
    for (pij_harness, native_harness) in [
        ("claude", "claude-code"),
        ("copilot", "copilot-cli"),
        ("codex", "codex"),
        ("omp", "oh-my-pi"),
        ("pi", "pi"),
    ] {
        set_response(
            &executable,
            &format!(
                r#"{{"v":2,"ok":true,"data":{{"id":"{ID}","harness":"{pij_harness}","session":"native-{pij_harness}"}}}}"#
            ),
        );
        let resolved = resolve(&executable).unwrap();
        assert_eq!(resolved.harness.as_str(), native_harness);
        assert_eq!(resolved.native_session_id, format!("native-{pij_harness}"));
    }

    set_response(
        &executable,
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":"codex","session":"replacement"}}"#,
    );
    assert_eq!(
        resolve(&executable).unwrap().native_session_id,
        "replacement"
    );
}

#[test]
fn unknown_seat_and_known_seat_without_native_identity_are_distinct() {
    let (_temporary, executable) = program(r#"{"v":2,"ok":false,"error":"not_found"}"#, 4);
    assert_eq!(resolve(&executable), Err(PijLookupError::UnknownSeat));

    set_response(
        &executable,
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":"omp","session":null}}"#,
    );
    fs::write(sidecar(&executable, ".status"), "0").unwrap();
    assert_eq!(
        resolve(&executable),
        Err(PijLookupError::NativeSessionUnavailable)
    );
}

#[test]
fn malformed_or_mismatched_identity_is_rejected_without_guessing() {
    let (_temporary, executable) = program("not json", 0);
    assert_eq!(resolve(&executable), Err(PijLookupError::InvalidResponse));

    for response in [
        r#"{"v":2,"ok":true,"data":{"id":"different-seat","harness":"omp","session":"native"}}"#,
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":"omp"}}"#,
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":7,"session":"native"}}"#,
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":"omp","session":"native","cwd":7}}"#,
    ] {
        set_response(&executable, response);
        assert_eq!(resolve(&executable), Err(PijLookupError::InvalidResponse));
    }

    set_response(
        &executable,
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":"other","session":"native"}}"#,
    );
    assert_eq!(
        resolve(&executable),
        Err(PijLookupError::UnsupportedHarness)
    );
}

#[test]
fn missing_executable_and_lookup_transport_failure_are_distinct() {
    assert_eq!(
        resolve_pij(None, ID, PijLookupLimits::default()),
        Err(PijLookupError::MissingExecutable)
    );
    assert_eq!(
        resolve_pij(
            Some(Path::new("/definitely/not/a/pij/executable")),
            ID,
            PijLookupLimits::default()
        ),
        Err(PijLookupError::MissingExecutable)
    );

    let (_temporary, executable) = program(r#"{"v":2,"ok":false,"error":"transport"}"#, 1);
    assert_eq!(resolve(&executable), Err(PijLookupError::LookupUnavailable));
}

#[test]
fn output_and_elapsed_time_are_bounded() {
    let response =
        r#"{"v":2,"ok":true,"data":{"id":"pij-test-seat","harness":"omp","session":"native"}}"#;
    let (_temporary, executable) = program(response, 0);
    let limits = PijLookupLimits {
        max_output_bytes: response.len(),
        timeout: Duration::from_secs(10),
    };
    assert_eq!(
        resolve_pij(Some(&executable), ID, limits)
            .unwrap()
            .native_session_id,
        "native"
    );
    assert_eq!(
        resolve_pij(
            Some(&executable),
            ID,
            PijLookupLimits {
                max_output_bytes: response.len() - 1,
                ..limits
            }
        ),
        Err(PijLookupError::OutputLimitExceeded)
    );

    fs::write(&executable, "#!/bin/sh\nexec sleep 30\n").unwrap();
    let started = Instant::now();
    assert_eq!(
        resolve_pij(
            Some(&executable),
            ID,
            PijLookupLimits {
                max_output_bytes: 1024,
                timeout: Duration::from_millis(50),
            }
        ),
        Err(PijLookupError::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn invalid_input_and_limits_fail_before_process_execution() {
    assert_eq!(
        resolve_pij(None, "", PijLookupLimits::default()),
        Err(PijLookupError::InvalidInput)
    );
    assert_eq!(
        resolve_pij(
            None,
            ID,
            PijLookupLimits {
                max_output_bytes: 0,
                timeout: Duration::from_secs(1),
            }
        ),
        Err(PijLookupError::InvalidLimits)
    );
    assert_eq!(
        resolve_pij(
            None,
            ID,
            PijLookupLimits {
                max_output_bytes: 1024,
                timeout: Duration::ZERO,
            }
        ),
        Err(PijLookupError::InvalidLimits)
    );
    assert_eq!(
        resolve_pij(
            None,
            ID,
            PijLookupLimits {
                max_output_bytes: usize::MAX,
                timeout: Duration::from_secs(1),
            }
        ),
        Err(PijLookupError::InvalidLimits)
    );
    assert_eq!(
        resolve_pij(
            None,
            ID,
            PijLookupLimits {
                max_output_bytes: 1024,
                timeout: Duration::MAX,
            }
        ),
        Err(PijLookupError::InvalidLimits)
    );
}

#[test]
fn absent_or_future_envelope_versions_cannot_resolve_identity() {
    let (_temporary, executable) = program("", 0);
    for prefix in ["", "\"v\":3,", "\"v\":\"2\","] {
        set_response(
            &executable,
            &format!(
                "{{{prefix}\"ok\":true,\"data\":{{\"id\":\"{ID}\",\"harness\":\"omp\",\"session\":\"native\"}}}}"
            ),
        );
        assert_eq!(resolve(&executable), Err(PijLookupError::InvalidResponse));
    }
}
