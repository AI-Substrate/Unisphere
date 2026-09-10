use super::{ProofResult, build, expect_status, require_file, run_product, strings};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};
use unisphere_testkit::collection::CLAUDE_BASIC;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Jsonl,
    Document,
    Journal,
    Sqlite,
}
impl Kind {
    fn flag(self) -> &'static str {
        match self {
            Self::Jsonl => "jsonl",
            Self::Document => "json-document",
            Self::Journal => "json-journal",
            Self::Sqlite => "sqlite-key-value",
        }
    }
    fn wire(self) -> &'static str {
        match self {
            Self::Jsonl => "jsonl",
            Self::Document => "json_document",
            Self::Journal => "json_journal",
            Self::Sqlite => "sqlite_key_value",
        }
    }
}
struct Case {
    id: &'static str,
    label: &'static str,
    input: PathBuf,
    kind: Kind,
}

fn consumer(repo: &Path, scratch: &Path) -> ProofResult<PathBuf> {
    let project = scratch.join("native-consumer");
    fs::create_dir_all(project.join("src")).map_err(|e| e.to_string())?;
    let mut manifest = "[package]\nname=\"unisphere-native-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\n".to_owned();
    for name in [
        "sdk",
        "loader-jsonl",
        "loader-snapshot",
        "adapter-claude",
        "adapter-codex",
        "adapter-omp",
        "adapter-pi",
        "adapter-copilot-cli",
        "adapter-cursor",
        "adapter-vscode-copilot",
        "output-otlp",
    ] {
        let path =
            serde_json::to_string(&repo.join("crates").join(name)).map_err(|e| e.to_string())?;
        manifest.push_str(&format!("unisphere-{name}={{path={path}}}\n"));
    }
    manifest.push_str("serde_json=\"1\"\nrusqlite={version=\"0.37\",features=[\"bundled\"]}\n");
    fs::write(project.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    fs::write(
        project.join("src/main.rs"),
        include_bytes!("../../../fixtures/consumer/native-main.rs"),
    )
    .map_err(|e| e.to_string())?;
    build(
        repo,
        &scratch.join("sdk-build"),
        &[
            OsStr::new("build"),
            OsStr::new("--manifest-path"),
            project.join("Cargo.toml").as_os_str(),
        ],
    )?;
    let binary = scratch.join("sdk-build/target/debug").join(format!(
        "unisphere-native-consumer{}",
        std::env::consts::EXE_SUFFIX
    ));
    require_file(&binary)?;
    Ok(binary)
}

fn args(case: &Case, content: bool) -> Vec<OsString> {
    let mut args = strings(&["sessions", "export", "--adapter", case.id, "--input"]);
    args.push(case.input.as_os_str().to_owned());
    if case.kind == Kind::Jsonl {
        args.extend(strings(&["--max-records", "1"]));
    } else {
        args.extend(strings(&["--source-format", case.kind.flag()]));
    }
    if content {
        args.push("--include-content".into());
    }
    args
}

fn sdk_args(case: &Case, content: bool) -> Vec<OsString> {
    let mut result = strings(&["export", case.id]);
    result.push(case.input.as_os_str().to_owned());
    result.extend(strings(&[
        if content { "content" } else { "metadata" },
        case.kind.flag(),
    ]));
    result
}

fn attr<'a>(record: &'a Value, key: &str) -> Option<&'a Value> {
    record["attributes"]
        .as_array()?
        .iter()
        .find(|item| item["key"] == key)
        .map(|item| &item["value"])
}

fn decode(bytes: &[u8], case: &Case, content: bool) -> ProofResult<Vec<Value>> {
    let records = super::collection::decode_records(bytes, case.kind != Kind::Jsonl)?;
    for record in &records {
        if attr(record, "unisphere.source.adapter").and_then(|v| v["stringValue"].as_str())
            != Some(case.id)
        {
            return Err(format!(
                "{} produced the wrong adapter identity",
                case.label
            ));
        }
        if !content && record.get("body").is_some() {
            return Err("metadata projection contains a body".into());
        }
        if case.kind != Kind::Jsonl
            && attr(record, "unisphere.source.format").and_then(|v| v["stringValue"].as_str())
                != Some(case.kind.wire())
        {
            return Err("snapshot representation differs from registered source kind".into());
        }
    }
    let wire = std::str::from_utf8(bytes).map_err(|_| "native output is not UTF-8")?;
    if content && !wire.contains("SENSITIVE") {
        return Err(format!("{} did not map its content fixture", case.label));
    }
    if !content && wire.contains("SENSITIVE") {
        return Err("metadata projection leaked fixture content".into());
    }
    if case.kind != Kind::Jsonl {
        let manifest = records.last().ok_or("snapshot has no closing manifest")?;
        if manifest["eventName"] != "unisphere.session.snapshot"
            || attr(manifest, "unisphere.snapshot.semantics")
                != Some(&json!({"stringValue":"replace_projection"}))
            || attr(manifest, "unisphere.snapshot.records")
                != Some(&json!({"intValue":(records.len()-1).to_string()}))
        {
            return Err("snapshot closing replacement manifest is invalid".into());
        }
    }
    Ok(records)
}

fn parity(
    cli: &Path,
    sdk: &Path,
    installed: &Path,
    scratch: &Path,
    case: &Case,
    content: bool,
) -> ProofResult<Vec<u8>> {
    let cli_result = run_product(cli, &scratch.join("cli"), &args(case, content))?;
    let sdk_result = run_product(sdk, &scratch.join("sdk"), &sdk_args(case, content))?;
    let installed_result =
        run_product(installed, &scratch.join("installed"), &args(case, content))?;
    expect_status(&cli_result, 0, case.label)?;
    expect_status(&sdk_result, 0, case.label)?;
    expect_status(&installed_result, 0, case.label)?;
    if cli_result.stdout != sdk_result.stdout
        || cli_result.stdout != installed_result.stdout
        || !sdk_result.stderr.is_empty()
    {
        return Err(format!("{} SDK/CLI/installed output mismatch", case.label));
    }
    decode(&cli_result.stdout, case, content)?;
    let summary: Value = serde_json::from_slice(&cli_result.stderr)
        .map_err(|_| "native export summary is not JSON")?;
    if summary["ok"] != true || summary["data"]["adapter"] != case.id {
        return Err("native export summary is wrong".into());
    }
    if case.kind != Kind::Jsonl
        && (summary["data"].get("offset").is_some() || !summary["data"]["revision"].is_string())
    {
        return Err("snapshot summary invents an append cursor".into());
    }
    Ok(cli_result.stdout)
}

fn revision(bytes: &[u8]) -> ProofResult<String> {
    let document: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let records = document["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
        .as_array()
        .ok_or("missing snapshot records")?;
    attr(
        records.last().ok_or("missing manifest")?,
        "unisphere.source.revision",
    )
    .and_then(|value| value["stringValue"].as_str())
    .map(str::to_owned)
    .ok_or("missing revision".into())
}

fn revisions(
    cli: &Path,
    sdk: &Path,
    installed: &Path,
    repo: &Path,
    scratch: &Path,
) -> ProofResult<()> {
    let original: Value = serde_json::from_slice(
        &fs::read(repo.join("crates/adapter-vscode-copilot/tests/fixtures/session-v3.json"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let document_path = scratch.join("mutable.json");
    fs::write(
        &document_path,
        serde_json::to_vec(&original).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let document_case = Case {
        id: "vscode-copilot",
        label: "changed-document",
        input: document_path.clone(),
        kind: Kind::Document,
    };
    let before = parity(
        cli,
        sdk,
        installed,
        &scratch.join("before"),
        &document_case,
        true,
    )?;
    let mut changed = original.clone();
    changed["requests"][0]["requestId"] = json!("revised-request");
    changed["requests"][0]["message"]["text"] = json!("SENSITIVE-REVISED-PROOF");
    fs::write(
        &document_path,
        serde_json::to_vec(&changed).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let updated = parity(
        cli,
        sdk,
        installed,
        &scratch.join("updated"),
        &document_case,
        true,
    )?;
    if revision(&before)? == revision(&updated)?
        || !String::from_utf8_lossy(&updated).contains("revised-request")
        || String::from_utf8_lossy(&updated).contains("SENSITIVE-USER-TEXT")
    {
        return Err("changed document retained stale projection".into());
    }
    changed["requests"] = json!([]);
    fs::write(
        &document_path,
        serde_json::to_vec(&changed).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let deleted = parity(
        cli,
        sdk,
        installed,
        &scratch.join("deleted"),
        &document_case,
        true,
    )?;
    if revision(&deleted)? == revision(&updated)?
        || String::from_utf8_lossy(&deleted).contains("revised-request")
    {
        return Err("deleted request survived replacement".into());
    }

    let journal_path = scratch.join("mutable.jsonl");
    let journal =
        fs::read(repo.join("crates/adapter-vscode-copilot/tests/fixtures/session-journal.jsonl"))
            .map_err(|e| e.to_string())?;
    fs::write(&journal_path, &journal).map_err(|e| e.to_string())?;
    let journal_case = Case {
        id: "vscode-copilot",
        label: "late-journal",
        input: journal_path.clone(),
        kind: Kind::Journal,
    };
    let before_journal = parity(
        cli,
        sdk,
        installed,
        &scratch.join("journal-before"),
        &journal_case,
        true,
    )?;
    let mut late = original["requests"][0].clone();
    late["requestId"] = json!("late-request");
    late["message"]["text"] = json!("SENSITIVE-LATE-PROOF");
    let mut appended = journal;
    let operation = serde_json::to_vec(&json!({"kind":2,"k":["requests"],"v":[late]}))
        .map_err(|e| e.to_string())?;
    appended.extend_from_slice(&operation[..operation.len() - 1]);
    fs::write(&journal_path, &appended).map_err(|e| e.to_string())?;
    let incomplete = run_product(cli, &scratch.join("partial"), &args(&journal_case, false))?;
    expect_status(&incomplete, 1, "partial journal")?;
    if !incomplete.stdout.is_empty() {
        return Err("partial journal published a prefix snapshot".into());
    }
    appended.extend_from_slice(b"}\n");
    fs::write(&journal_path, &appended).map_err(|e| e.to_string())?;
    let late_journal = parity(
        cli,
        sdk,
        installed,
        &scratch.join("journal-late"),
        &journal_case,
        true,
    )?;
    if revision(&before_journal)? == revision(&late_journal)?
        || !String::from_utf8_lossy(&late_journal).contains("late-request")
    {
        return Err("late journal append was lost".into());
    }

    let db = scratch.join("mutable.db");
    let seed = vec![
        OsString::from("seed-cursor"),
        repo.join("crates/adapter-cursor/fixtures/ide.json")
            .into_os_string(),
        db.clone().into_os_string(),
    ];
    expect_status(
        &run_product(sdk, &scratch.join("seed"), &seed)?,
        0,
        "seed native SQLite",
    )?;
    let db_case = Case {
        id: "cursor-ide",
        label: "mutable-sqlite",
        input: db.clone(),
        kind: Kind::Sqlite,
    };
    let mut previous = revision(&parity(
        cli,
        sdk,
        installed,
        &scratch.join("db-before"),
        &db_case,
        true,
    )?)?;
    for action in ["late", "update", "delete"] {
        let mutation = vec![
            OsString::from("change-cursor"),
            db.clone().into_os_string(),
            OsString::from(action),
        ];
        expect_status(
            &run_product(sdk, &scratch.join(format!("mutate-{action}")), &mutation)?,
            0,
            "native SQLite mutation",
        )?;
        // An empty table has no content marker, so use metadata mode for its replacement.
        let result = parity(
            cli,
            sdk,
            installed,
            &scratch.join(format!("db-{action}")),
            &db_case,
            action != "delete",
        )?;
        let next = revision(&result)?;
        if next == previous {
            return Err("SQLite mutation did not change revision".into());
        }
        previous = next;
        let wire = String::from_utf8_lossy(&result);
        if action == "late" && !wire.contains("SENSITIVE-LATE-PROOF") {
            return Err("late SQLite row missing".into());
        }
        if action == "update"
            && (!wire.contains("SENSITIVE-REVISED-PROOF") || wire.contains("SENSITIVE-ANSWER"))
        {
            return Err("SQLite update retained stale value".into());
        }
        if action == "delete" && decode(&result, &db_case, false)?.len() != 1 {
            return Err("empty SQLite replacement retained records".into());
        }
    }
    let failure = run_product(
        sdk,
        &scratch.join("failure"),
        &[
            OsString::from("failure-probe"),
            document_path.clone().into_os_string(),
        ],
    )?;
    expect_status(&failure, 0, "real output failure checkpoint")?;
    let failure: Value = serde_json::from_slice(&failure.stdout).map_err(|e| e.to_string())?;
    if failure["checkpoint_on_failure"] != false || failure["partial_bytes"] != 11 {
        return Err("output failure published checkpoint".into());
    }
    fs::remove_file(&document_path).map_err(|e| e.to_string())?;
    let missing = run_product(cli, &scratch.join("missing"), &args(&document_case, false))?;
    expect_status(&missing, 1, "deleted source file")?;
    if !missing.stdout.is_empty() {
        return Err("missing source produced fake successful output".into());
    }
    Ok(())
}

pub fn run(repo: &Path, scratch: &Path) -> ProofResult<()> {
    let sdk = consumer(repo, scratch)?;
    let build_root = scratch.join("cli-build");
    build(
        repo,
        &build_root,
        &[
            OsStr::new("build"),
            OsStr::new("--locked"),
            OsStr::new("-p"),
            OsStr::new("unisphere-app"),
        ],
    )?;
    let cli = build_root
        .join("target/debug")
        .join(format!("unisphere{}", std::env::consts::EXE_SUFFIX));
    let install = scratch.join("install");
    build(
        repo,
        &build_root,
        &[
            OsStr::new("install"),
            OsStr::new("--locked"),
            OsStr::new("--debug"),
            OsStr::new("--path"),
            repo.join("crates/app").as_os_str(),
            OsStr::new("--root"),
            install.as_os_str(),
        ],
    )?;
    let installed = install
        .join("bin")
        .join(format!("unisphere{}", std::env::consts::EXE_SUFFIX));
    require_file(&installed)?;
    let claude = scratch.join("claude.jsonl");
    fs::write(&claude, CLAUDE_BASIC).map_err(|e| e.to_string())?;
    let db = scratch.join("cursor.db");
    expect_status(
        &run_product(
            &sdk,
            &scratch.join("seed"),
            &[
                OsString::from("seed-cursor"),
                repo.join("crates/adapter-cursor/fixtures/ide.json")
                    .into_os_string(),
                db.clone().into_os_string(),
            ],
        )?,
        0,
        "seed cursor database",
    )?;
    let cases = [
        Case {
            id: "claude-code",
            label: "claude",
            input: claude,
            kind: Kind::Jsonl,
        },
        Case {
            id: "codex",
            label: "codex",
            input: repo.join("crates/adapter-codex/fixtures/rollout.jsonl"),
            kind: Kind::Jsonl,
        },
        Case {
            id: "oh-my-pi",
            label: "omp",
            input: repo.join("crates/adapter-omp/tests/fixtures/native.jsonl"),
            kind: Kind::Jsonl,
        },
        Case {
            id: "pi",
            label: "pi",
            input: repo.join("crates/adapter-pi/tests/fixtures/v3-tree.jsonl"),
            kind: Kind::Jsonl,
        },
        Case {
            id: "copilot-cli",
            label: "copilot",
            input: repo.join("crates/adapter-copilot-cli/tests/fixtures/events.jsonl"),
            kind: Kind::Jsonl,
        },
        Case {
            id: "cursor-transcript",
            label: "cursor-transcript",
            input: repo.join("crates/adapter-cursor/fixtures/transcript.jsonl"),
            kind: Kind::Jsonl,
        },
        Case {
            id: "vscode-copilot",
            label: "vscode-document",
            input: repo.join("crates/adapter-vscode-copilot/tests/fixtures/session-v3.json"),
            kind: Kind::Document,
        },
        Case {
            id: "vscode-copilot",
            label: "vscode-journal",
            input: repo.join("crates/adapter-vscode-copilot/tests/fixtures/session-journal.jsonl"),
            kind: Kind::Journal,
        },
        Case {
            id: "copilot-cli-snapshot",
            label: "copilot-snapshot",
            input: repo.join("crates/adapter-copilot-cli/tests/fixtures/legacy.json"),
            kind: Kind::Document,
        },
        Case {
            id: "cursor-ide",
            label: "cursor-ide",
            input: db,
            kind: Kind::Sqlite,
        },
    ];
    let catalog = run_product(
        &installed,
        &scratch.join("catalog"),
        &strings(&["adapters", "list", "--json"]),
    )?;
    expect_status(&catalog, 0, "installed native catalog")?;
    let catalog: Value = serde_json::from_slice(&catalog.stdout).map_err(|e| e.to_string())?;
    let ids: BTreeSet<_> = catalog["data"]["adapters"]
        .as_array()
        .ok_or("catalog lacks adapters")?
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    // Git objects have their own external-consumer lane, not a session-file decoder case.
    if ids != cases.iter().map(|case| case.id).chain(["git-ai"]).collect() {
        return Err("catalog and real native/Git Notes proof cases differ".into());
    }
    for case in &cases {
        for content in [false, true] {
            parity(
                &cli,
                &sdk,
                &installed,
                &scratch.join(format!("{}-{content}", case.label)),
                case,
                content,
            )?;
        }
    }
    let revision_root = scratch.join("revisions");
    fs::create_dir_all(&revision_root).map_err(|e| e.to_string())?;
    revisions(&cli, &sdk, &installed, repo, &revision_root)?;
    super::query::run(&cli, &installed, &scratch.join("queries"))?;
    println!(
        "native proof: all registered dialects SDK/CLI/installed parity; changed/deleted/late sources; partial output without checkpoint"
    );
    Ok(())
}
