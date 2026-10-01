//! Assembled session-status proof (vd-0006): the built and installed CLI and an
//! external SDK consumer with a caller-held cursor answer for the same
//! synthetic Claude session; the harness-neutral facts must agree.

use super::{ProofResult, build, require_file, run_product, strings};
use serde_json::{Value, json};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const FILE: &str = "-Users-dev-demo/sess-0001.jsonl";
const SESSION: &str = "sess-0001";
/// A content-free main-chain call on a new model, appended in two writes.
const APPENDED: &str = r#"{"type":"assistant","timestamp":"2026-09-26T09:00:00Z","sessionId":"sess-0001","isSidechain":false,"requestId":"req_status_append","message":{"id":"msg_status_append","model":"claude-proof-2","stop_reason":"end_turn","usage":{"input_tokens":7,"cache_read_input_tokens":11,"cache_creation_input_tokens":0,"output_tokens":3}}}"#;

fn fixture(repo: &Path) -> PathBuf {
    repo.join("crates/adapter-claude/tests/fixtures/prep")
        .join(FILE)
}

/// Seed the fixture where the catalogue default root puts Claude sessions.
fn seed(repo: &Path, root: &Path) -> ProofResult<PathBuf> {
    let _ = fs::remove_dir_all(root);
    let to = root.join(FILE);
    fs::create_dir_all(to.parent().ok_or("fixture has no parent")?).map_err(|e| e.to_string())?;
    fs::copy(fixture(repo), &to).map_err(|e| e.to_string())?;
    Ok(to)
}

fn append(path: &Path, bytes: &[u8]) -> ProofResult<()> {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| e.to_string())
}

fn status(binary: &Path, sandbox: &Path, args: &[&str]) -> ProofResult<(i32, Value)> {
    let mut argv = strings(&["sessions", "status"]);
    argv.extend(strings(args));
    argv.push(OsString::from("--json"));
    let output = run_product(binary, sandbox, &argv)?;
    let code = output
        .status
        .code()
        .ok_or("status terminated by a signal")?;
    let value = serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "status output is not JSON ({e}): {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    Ok((code, value))
}

/// The same projection the SDK consumer prints.
fn project(status: &Value) -> Value {
    json!({
        "target": status["target"]["session_id"],
        "model": status["model"]["current"]["value"],
        "used_tokens": status["context"]["used_tokens"]["value"],
        "calls": status["calls"]["total"],
        "turns": status["turns"]["total"],
        "compactions": status["compaction"]["counts"],
        "unknown": status["unknown"],
    })
}

fn check(condition: bool, label: &str, value: &Value) -> ProofResult<()> {
    if condition {
        Ok(())
    } else {
        Err(format!("{label}: unexpected status {value}"))
    }
}

fn failure_code(value: &Value) -> &Value {
    &value["data"]["results"][0]["error"]["code"]
}

/// One scenario against a CLI binary; returns the comparable projections.
fn scenario(repo: &Path, binary: &Path, dir: &Path) -> ProofResult<Vec<Value>> {
    let sandbox = dir.join("sandbox");
    let file = seed(repo, &sandbox.join("home/.claude/projects"))?;
    let by_id = ["--session", SESSION, "--harness", "claude-code"];
    let mut steps = Vec::new();

    let (code, cold) = status(binary, &sandbox, &by_id)?;
    let facts = &cold["data"]["results"][0]["status"];
    check(
        code == 0
            && facts["calls"]["total"].as_u64() > Some(0)
            && facts["model"]["current"]["basis"] == "native"
            && facts["compaction"]["counts"].is_object()
            && facts["resolved"]["basis"] == "explicit"
            && facts["target"]["transcript"]
                .as_str()
                .is_some_and(|path| path.ends_with(FILE)),
        "cold status by session id",
        &cold,
    )?;
    steps.push(project(facts));

    let (head, tail) = APPENDED.split_at(APPENDED.len() / 2);
    append(&file, head.as_bytes())?;
    let (code, partial) = status(binary, &sandbox, &by_id)?;
    let facts = &partial["data"]["results"][0]["status"];
    check(
        code == 0
            && facts["source"]["pending_tail_bytes"].as_u64() >= Some(head.len() as u64)
            && facts["calls"]["total"] == steps[0]["calls"],
        "partial tail is pending, never an error",
        &partial,
    )?;

    append(&file, format!("{tail}\n").as_bytes())?;
    let (code, appended) = status(binary, &sandbox, &by_id)?;
    let facts = &appended["data"]["results"][0]["status"];
    check(
        code == 0
            && facts["model"]["current"]["value"] == "claude-proof-2"
            && facts["calls"]["total"].as_u64()
                == steps[0]["calls"].as_u64().map(|calls| calls + 1),
        "completed record switches the current model to the newest call",
        &appended,
    )?;
    steps.push(project(facts));

    let (code, value) = status(binary, &sandbox, &["--session", "x", "--harness", "git-ai"])?;
    check(
        code == 3 && failure_code(&value) == "UNI-STATUS-UNSUPPORTED-HARNESS",
        "unsupported harness",
        &value,
    )?;
    let (code, value) = status(
        binary,
        &sandbox,
        &["--session", "nope", "--harness", "claude-code"],
    )?;
    check(
        code == 3 && failure_code(&value) == "UNI-STATUS-TRANSCRIPT-NOT-FOUND",
        "unknown session",
        &value,
    )?;
    // The sealed product environment has an empty PATH: no pij, no tmux.
    let (code, value) = status(binary, &sandbox, &["--pij", "pij-proof-seat"])?;
    check(
        code == 3 && failure_code(&value) == "UNI-STATUS-PIJ-UNAVAILABLE",
        "pij unavailable",
        &value,
    )?;
    Ok(steps)
}

fn consumer(repo: &Path, scratch: &Path) -> ProofResult<PathBuf> {
    let project = scratch.join("status-consumer");
    fs::create_dir_all(project.join("src")).map_err(|e| e.to_string())?;
    let mut manifest = "[package]\nname=\"unisphere-status-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\n".to_owned();
    for name in ["sdk", "loader-jsonl", "adapter-claude"] {
        let path =
            serde_json::to_string(&repo.join("crates").join(name)).map_err(|e| e.to_string())?;
        manifest.push_str(&format!("unisphere-{name}={{path={path}}}\n"));
    }
    manifest.push_str("serde_json=\"1\"\n");
    fs::write(project.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    fs::write(
        project.join("src/main.rs"),
        include_bytes!("../../../fixtures/consumer/status-main.rs"),
    )
    .map_err(|e| e.to_string())?;
    build(
        repo,
        &scratch.join("status-consumer-build"),
        &[
            OsStr::new("build"),
            OsStr::new("--manifest-path"),
            project.join("Cargo.toml").as_os_str(),
        ],
    )?;
    let binary = scratch
        .join("status-consumer-build/target/debug")
        .join(format!(
            "unisphere-status-consumer{}",
            env::consts::EXE_SUFFIX
        ));
    require_file(&binary)?;
    Ok(binary)
}

pub fn run(repo: &Path, scratch: &Path) -> ProofResult<()> {
    require_file(&fixture(repo))?;
    let install = scratch.join("install");
    build(
        repo,
        &scratch.join("build"),
        &[
            OsStr::new("install"),
            OsStr::new("--locked"),
            OsStr::new("--path"),
            repo.join("crates/app").as_os_str(),
            OsStr::new("--root"),
            install.as_os_str(),
        ],
    )?;
    let installed = install
        .join("bin")
        .join(format!("unisphere{}", env::consts::EXE_SUFFIX));
    build(
        repo,
        &scratch.join("build"),
        &[
            OsStr::new("build"),
            OsStr::new("--locked"),
            OsStr::new("-p"),
            OsStr::new("unisphere-app"),
        ],
    )?;
    let built = scratch
        .join("build/target/debug")
        .join(format!("unisphere{}", env::consts::EXE_SUFFIX));
    require_file(&built)?;
    require_file(&installed)?;
    let from_built = scenario(repo, &built, &scratch.join("built"))?;
    let from_installed = scenario(repo, &installed, &scratch.join("installed"))?;
    if from_built != from_installed {
        return Err(format!(
            "built and installed CLI disagree:\n{}\n{}",
            Value::from(from_built),
            Value::from(from_installed)
        ));
    }

    // External SDK consumer: same session, its own cursor, same facts.
    let sdk = consumer(repo, scratch)?;
    let root = scratch.join("consumer-root");
    seed(repo, &root)?;
    let output = run_product(
        &sdk,
        &scratch.join("consumer-sandbox"),
        &[root.into(), SESSION.into(), APPENDED.into()],
    )?;
    super::expect_status(&output, 0, "external SDK status consumer")?;
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    if value["cold"] != from_built[0]
        || value["incremental"] != from_built[1]
        || value["cold_after"] != value["incremental"]
        || value["unchanged_bytes_read"] != 0
        || value["incremental_bytes_read"].as_u64() >= value["cold_after_bytes_read"].as_u64()
    {
        return Err(format!(
            "SDK consumer disagrees with the CLI: consumer={value} cli={}",
            Value::from(from_built)
        ));
    }
    Ok(())
}
