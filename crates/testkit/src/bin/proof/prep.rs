//! Assembled prep proof (vd-0008): an external SDK consumer with its own store
//! and the built and installed CLI run the same incremental prep over synthetic
//! Claude sources; every observable step must agree.

use super::{ProofResult, build, expect_status, require_file, run_product, strings};
use serde_json::{Value, json};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const MAIN: &str = "-Users-dev-demo/sess-0001.jsonl";
const SUB: &str = "-Users-dev-demo/sess-0001/subagents/agent-a1.jsonl";
const SOURCE: &str = "claude-code/demo/-Users-dev-demo/sess-0001.jsonl";
/// A content-free call record appended in two writes to prove partial tails.
const APPENDED: &str = r#"{"type":"assistant","timestamp":"2026-09-26T09:00:00Z","sessionId":"sess-0001","requestId":"req_proof_append","message":{"id":"msg_proof_append","model":"claude-proof","stop_reason":"end_turn","usage":{"input_tokens":7,"cache_read_input_tokens":11,"output_tokens":3}}}"#;

fn fixtures(repo: &Path) -> PathBuf {
    repo.join("crates/adapter-claude/tests/fixtures/prep")
}

/// Fresh copy of the synthetic Claude root.
fn seed_root(repo: &Path, root: &Path) -> ProofResult<()> {
    let _ = fs::remove_dir_all(root);
    for file in [MAIN, SUB] {
        let to = root.join(file);
        fs::create_dir_all(to.parent().ok_or("fixture has no parent")?)
            .map_err(|e| e.to_string())?;
        fs::copy(fixtures(repo).join(file), &to).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn append(path: &Path, bytes: &[u8]) -> ProofResult<()> {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| e.to_string())
}

fn prep_json(binary: &Path, sandbox: &Path, args: &[OsString]) -> ProofResult<(i32, Value)> {
    let output = run_product(binary, sandbox, args)?;
    let code = output.status.code().ok_or("prep terminated by a signal")?;
    let value = if output.stdout.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&output.stdout).map_err(|e| {
            format!(
                "prep output is not JSON ({e}): {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })?
    };
    Ok((code, value))
}

fn run_args(target: &Path, root: &Path) -> Vec<OsString> {
    let mut args = strings(&["prep", "--target"]);
    args.push(target.into());
    args.push("--root".into());
    let mut spec = OsString::from("claude-code:demo=");
    spec.push(root);
    args.push(spec);
    args.extend(strings(&["--no-default-roots", "--json"]));
    args
}

/// Comparable facts of one prep report, without paths or timings.
fn project(report: &Value) -> Value {
    let data = &report["data"];
    json!({
        "command": report["command"],
        "run": data["run"],
        "bytes_read": data["bytes_read"],
        "pending_tail_bytes": data["pending_tail_bytes"],
        "rows_written": data["rows_written"],
        "sets": data["sets"].as_array().map(|sets| sets.iter().map(|s| json!({
            "harness": s["harness"], "label": s["label"], "supported": s["supported"],
            "discovered": s["discovered"], "skipped": s["skipped"], "by_status": s["by_status"],
        })).collect::<Vec<_>>()),
        "sources": data["sources"].as_array().map(|sources| sources.iter().map(|s| json!({
            "source": s["source"], "status": s["status"], "generation": s["generation"],
            "rows": s["rows"], "pending_tail_bytes": s["pending_tail_bytes"],
        })).collect::<Vec<_>>()),
        "orphans_removed": data["commit"]["orphans_removed"],
    })
}

fn expect(condition: bool, label: &str, value: &Value) -> ProofResult<()> {
    if condition {
        Ok(())
    } else {
        Err(format!("{label}: unexpected report {value}"))
    }
}

/// One full scenario against a CLI binary; returns the comparable projections.
fn scenario(repo: &Path, binary: &Path, dir: &Path) -> ProofResult<Vec<Value>> {
    let sandbox = dir.join("sandbox");
    let root = dir.join("root");
    let target = dir.join("target");
    seed_root(repo, &root)?;
    let args = run_args(&target, &root);
    let mut steps = Vec::new();
    let mut step = |label: &str, code: i32| -> ProofResult<Value> {
        let (actual, value) = prep_json(binary, &sandbox, &args)?;
        if actual != code {
            return Err(format!(
                "{label}: expected exit {code}, got {actual}: {value}"
            ));
        }
        steps.push(project(&value));
        Ok(value)
    };

    let cold = step("cold", 0)?;
    expect(
        cold["data"]["sets"][0]["by_status"]["new"] == 2
            && cold["data"]["rows_written"]["calls"].as_u64() > Some(0),
        "cold",
        &cold,
    )?;
    let unchanged = step("unchanged", 0)?;
    expect(
        unchanged["data"]["bytes_read"] == 0
            && unchanged["data"]["run"] == cold["data"]["run"]
            && unchanged["data"]["sets"][0]["by_status"]["unchanged"] == 2,
        "unchanged run reads nothing and commits nothing",
        &unchanged,
    )?;
    let main = root.join(MAIN);
    let (head, tail) = APPENDED.split_at(APPENDED.len() / 2);
    append(&main, head.as_bytes())?;
    let partial = step("partial tail", 0)?;
    expect(
        partial["data"]["pending_tail_bytes"].as_u64() >= Some(head.len() as u64)
            && partial["data"]["rows_written"]["calls"] == 0,
        "partial tail is pending, not read",
        &partial,
    )?;
    append(&main, format!("{tail}\n").as_bytes())?;
    let appended = step("append", 0)?;
    expect(
        appended["data"]["sets"][0]["by_status"]["appended"] == 1
            && appended["data"]["rows_written"]["calls"] == 1
            && appended["data"]["pending_tail_bytes"] == 0,
        "completed tail contributes exactly its record",
        &appended,
    )?;
    let rotated = root.join("rotated.tmp");
    fs::copy(&main, &rotated).map_err(|e| e.to_string())?;
    fs::rename(&rotated, &main).map_err(|e| e.to_string())?;
    let replaced = step("rotation", 0)?;
    expect(
        replaced["data"]["sources"].as_array().is_some_and(|s| {
            s.iter().any(|s| {
                s["status"]["status"] == "replaced"
                    && s["status"]["reason"] == "rotated"
                    && s["generation"] == 1
            })
        }),
        "rotated source starts generation 1",
        &replaced,
    )?;
    let orphan = target.join("tables/calls/run-999999.parquet");
    fs::write(&orphan, b"crashed-run-leftover").map_err(|e| e.to_string())?;
    let recovered = step("orphan recovery", 0)?;
    expect(
        recovered["data"]["commit"]["orphans_removed"].as_u64() >= Some(1) && !orphan.exists(),
        "crashed-run part is removed on load",
        &recovered,
    )?;

    let mut compact = strings(&["prep", "compact", "--target"]);
    compact.push(target.as_os_str().into());
    compact.push("--json".into());
    let (code, value) = prep_json(binary, &sandbox, &compact)?;
    if code != 0 || value["command"] != "prep.compact" {
        return Err(format!("compact: exit {code}: {value}"));
    }
    let after = step("after compaction", 0)?;
    expect(
        after["data"]["rows_written"]["calls"] == 0 && after["data"]["bytes_read"] == 0,
        "compaction leaves nothing to re-read",
        &after,
    )?;

    let mut record = strings(&["prep", "record", "--target"]);
    record.push(target.as_os_str().into());
    record.extend(strings(&["--source", SOURCE, "--offset", "0", "--json"]));
    let refused = run_product(binary, &sandbox, &record)?;
    expect_status(&refused, 2, "record without --include-content")?;
    record.push("--include-content".into());
    let (code, value) = prep_json(binary, &sandbox, &record)?;
    let first_line = fs::read_to_string(&main)
        .map_err(|e| e.to_string())?
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    if code != 0 || value["data"]["record"] != first_line.as_str() {
        return Err(format!("record fetch: exit {code}: {value}"));
    }
    steps.push(json!({"record_bytes": value["data"]["bytes"]}));

    let mut nothing = strings(&["prep", "--target"]);
    nothing.push(dir.join("empty-target").into());
    nothing.push("--json".into());
    let empty = run_product(binary, &sandbox, &nothing)?;
    expect_status(&empty, 2, "no default root exists and none was given")?;
    Ok(steps)
}

/// One snapshot representation driven through the CLI: prepped by revision,
/// unchanged when rewritten with equal content, a new generation when changed.
struct SnapshotCase {
    harness: &'static str,
    /// Source path below the root.
    file: &'static str,
    /// Synthetic repository fixture holding the first revision.
    first: &'static str,
    /// The changed revision's bytes, from the first revision's.
    changed: fn(&Path, &[u8]) -> ProofResult<Vec<u8>>,
    /// A native key whose record `prep record` must return, and a fragment of it.
    record: Option<(&'static str, &'static str)>,
}

const SNAPSHOT_CASES: [SnapshotCase; 3] = [
    // Whole JSON document (Copilot CLI legacy session).
    SnapshotCase {
        harness: "copilot-cli-snapshot",
        file: "legacy-0001.json",
        first: "crates/adapter-copilot-cli/tests/fixtures/prep/legacy-0001.json",
        changed: |_, bytes| {
            let mut document: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            document["chatMessages"]
                .as_array_mut()
                .ok_or("fixture has no chatMessages")?
                .push(json!({"role": "user", "content": "placeholder appended prompt"}));
            Ok(document.to_string().into_bytes())
        },
        record: Some(("document#/chatMessages/0", "\"role\"")),
    },
    // Native mutation journal (VS Code Copilot), replayed to its document.
    SnapshotCase {
        harness: "vscode-copilot",
        file: "ws-0001/chatSessions/sess-prep.jsonl",
        first: "crates/adapter-vscode-copilot/tests/fixtures/prep/ws-0001/chatSessions/sess-prep.jsonl",
        changed: |_, bytes| {
            let mut journal = bytes.to_vec();
            journal.extend_from_slice(
                b"{\"kind\":1,\"k\":[\"customTitle\"],\"v\":\"placeholder changed title\"}\n",
            );
            Ok(journal)
        },
        record: None,
    },
    // SQLite key/value store (Cursor IDE); the change adds a row.
    SnapshotCase {
        harness: "cursor-ide",
        file: "state.vscdb",
        first: "crates/testkit/fixtures/prep-snapshots/state-v1.vscdb",
        changed: |repo, _| {
            fs::read(repo.join("crates/testkit/fixtures/prep-snapshots/state-v2.vscdb"))
                .map_err(|e| e.to_string())
        },
        record: Some(("bubbleId:c-main:u1", "\"bubbleId\"")),
    },
];

/// Every snapshot representation through the same CLI: a JSON document, a
/// mutation journal and a SQLite store are each prepped, unchanged by an
/// equal-content rewrite, replaced as generation 1 on a content change, and
/// (where the native key is fetchable) addressable by `prep record`.
fn snapshot_scenarios(repo: &Path, binary: &Path, dir: &Path) -> ProofResult<Vec<Value>> {
    let mut steps = Vec::new();
    for case in &SNAPSHOT_CASES {
        steps.extend(snapshot_case(repo, binary, dir, case)?);
    }
    Ok(steps)
}

fn snapshot_case(
    repo: &Path,
    binary: &Path,
    dir: &Path,
    case: &SnapshotCase,
) -> ProofResult<Vec<Value>> {
    let sandbox = dir.join("sandbox");
    let root = dir.join(format!("{}-root", case.harness));
    let target = dir.join(format!("{}-target", case.harness));
    let _ = fs::remove_dir_all(&root);
    let source = root.join(case.file);
    fs::create_dir_all(source.parent().ok_or("source has no parent")?)
        .map_err(|e| e.to_string())?;
    let first = fs::read(repo.join(case.first)).map_err(|e| e.to_string())?;
    fs::write(&source, &first).map_err(|e| e.to_string())?;
    let mut args = strings(&["prep", "--target"]);
    args.push(target.as_os_str().into());
    let mut spec = OsString::from(format!("{}:demo=", case.harness));
    spec.push(&root);
    args.push("--root".into());
    args.push(spec);
    args.extend(strings(&["--no-default-roots", "--json"]));
    let mut steps = Vec::new();
    let mut step = |label: &str| -> ProofResult<Value> {
        let (code, value) = prep_json(binary, &sandbox, &args)?;
        if code != 0 {
            return Err(format!("{} {label}: exit {code}: {value}", case.harness));
        }
        steps.push(project(&value));
        Ok(value)
    };
    let rows = |value: &Value| -> u64 {
        value["data"]["rows_written"]
            .as_object()
            .map_or(0, |rows| rows.values().filter_map(Value::as_u64).sum())
    };
    let cold = step("cold")?;
    expect(
        cold["data"]["sets"][0]["supported"] == true
            && cold["data"]["sets"][0]["by_status"]["new"] == 1
            && rows(&cold) > 0,
        "snapshot source is prepped",
        &cold,
    )?;
    // Same bytes, new mtime: re-read, equal revision, nothing written.
    fs::write(&source, &first).map_err(|e| e.to_string())?;
    let touched = step("touched")?;
    expect(
        touched["data"]["sets"][0]["by_status"]["unchanged"] == 1 && rows(&touched) == 0,
        "equal revision is unchanged",
        &touched,
    )?;
    fs::write(&source, (case.changed)(repo, &first)?).map_err(|e| e.to_string())?;
    let replaced = step("changed")?;
    expect(
        replaced["data"]["sources"].as_array().is_some_and(|s| {
            s.iter().any(|s| {
                s["status"]["status"] == "replaced"
                    && s["status"]["reason"] == "revision"
                    && s["generation"] == 1
            })
        }),
        "changed snapshot starts generation 1",
        &replaced,
    )?;
    if let Some((key, fragment)) = case.record {
        let mut record = strings(&["prep", "record", "--target"]);
        record.push(target.as_os_str().into());
        record.push("--source".into());
        record.push(format!("{}/demo/{}", case.harness, case.file).into());
        record.extend(strings(&["--key", key, "--include-content", "--json"]));
        let (code, value) = prep_json(binary, &sandbox, &record)?;
        if code != 0
            || value["data"]["record"]
                .as_str()
                .is_none_or(|r| !r.contains(fragment))
        {
            return Err(format!(
                "{} record fetch: exit {code}: {value}",
                case.harness
            ));
        }
    }
    Ok(steps)
}

fn consumer(repo: &Path, scratch: &Path) -> ProofResult<PathBuf> {
    let project = scratch.join("prep-consumer");
    fs::create_dir_all(project.join("src")).map_err(|e| e.to_string())?;
    let mut manifest = "[package]\nname=\"unisphere-prep-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\n".to_owned();
    for name in ["sdk", "loader-jsonl", "adapter-claude"] {
        let path =
            serde_json::to_string(&repo.join("crates").join(name)).map_err(|e| e.to_string())?;
        manifest.push_str(&format!("unisphere-{name}={{path={path}}}\n"));
    }
    manifest.push_str("serde_json=\"1\"\n");
    fs::write(project.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    fs::write(
        project.join("src/main.rs"),
        include_bytes!("../../../fixtures/consumer/prep-main.rs"),
    )
    .map_err(|e| e.to_string())?;
    build(
        repo,
        &scratch.join("prep-consumer-build"),
        &[
            OsStr::new("build"),
            OsStr::new("--manifest-path"),
            project.join("Cargo.toml").as_os_str(),
        ],
    )?;
    let binary = scratch
        .join("prep-consumer-build/target/debug")
        .join(format!(
            "unisphere-prep-consumer{}",
            env::consts::EXE_SUFFIX
        ));
    require_file(&binary)?;
    Ok(binary)
}

pub fn run(repo: &Path, scratch: &Path) -> ProofResult<()> {
    require_file(&fixtures(repo).join(MAIN))?;
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
    let mut from_built = scenario(repo, &built, &scratch.join("built"))?;
    let mut from_installed = scenario(repo, &installed, &scratch.join("installed"))?;
    from_built.extend(snapshot_scenarios(repo, &built, &scratch.join("built"))?);
    from_installed.extend(snapshot_scenarios(
        repo,
        &installed,
        &scratch.join("installed"),
    )?);
    if from_built != from_installed {
        return Err(format!(
            "built and installed CLI disagree:\n{}\n{}",
            Value::from(from_built),
            Value::from(from_installed)
        ));
    }

    // External SDK consumer: same sources, its own store, same outcome.
    let sdk = consumer(repo, scratch)?;
    let root = scratch.join("consumer-root");
    seed_root(repo, &root)?;
    let output = run_product(
        &sdk,
        &scratch.join("consumer-sandbox"),
        &[root.as_os_str().into(), root.join(MAIN).into()],
    )?;
    expect_status(&output, 0, "external SDK prep consumer")?;
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    let cli_cold = &from_built[0];
    let first = &value["runs"][0];
    let second = &value["runs"][1];
    if first["rows_written"] != cli_cold["rows_written"]
        || first["by_status"] != json!([cli_cold["sets"][0]["by_status"]])
        || second["bytes_read"] != 0
        || second["run"] != first["run"]
        || value["committed_rows"].as_u64() != Some(cli_rows(cli_cold))
        || value["fold_source"]["calls"].as_u64() == Some(0)
        || value["fold_source"]["compactions_known"] != true
    {
        return Err(format!(
            "SDK consumer disagrees with the CLI: consumer={value} cli_cold={cli_cold}"
        ));
    }
    Ok(())
}

fn cli_rows(cold: &Value) -> u64 {
    let rows = &cold["rows_written"];
    ["calls", "turns", "triggers", "events", "tool_uses"]
        .iter()
        .filter_map(|table| rows[*table].as_u64())
        .sum()
}
