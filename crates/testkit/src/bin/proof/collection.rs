use super::{ProofResult, build, capture, expect_status, require_file, run_product, strings};
use serde_json::Value;
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};
use unisphere_testkit::{
    collection::{CLAUDE_BASIC, CLAUDE_PARTS},
    sealed_command,
};

fn decode_records(bytes: &[u8]) -> ProofResult<Vec<Value>> {
    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        return Err("OTLP output lacks final LF".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "OTLP output is not UTF-8")?;
    let mut records = Vec::new();
    for line in text.lines() {
        let doc: Value =
            serde_json::from_str(line).map_err(|e| format!("invalid OTLP JSON: {e}"))?;
        let object = doc.as_object().ok_or("OTLP line is not an object")?;
        if object.len() != 1 || !object.contains_key("resourceLogs") {
            return Err("non-OTLP top-level keys".into());
        }
        for resource in doc["resourceLogs"]
            .as_array()
            .ok_or("missing resourceLogs")?
        {
            for scope in resource["scopeLogs"]
                .as_array()
                .ok_or("missing scopeLogs")?
            {
                for record in scope["logRecords"].as_array().ok_or("missing logRecords")? {
                    if record["eventName"] != "unisphere.session.record" {
                        return Err("wrong source-derived event category".into());
                    }
                    if record
                        .get("timeUnixNano")
                        .is_some_and(|time| !time.is_string())
                    {
                        return Err("OTLP timestamp is not a decimal string".into());
                    }
                    if record.get("traceId").is_some() || record.get("spanId").is_some() {
                        return Err("unexpected invented trace correlation".into());
                    }
                    let attrs = record["attributes"]
                        .as_array()
                        .ok_or("missing provenance attributes")?;
                    for key in [
                        "unisphere.source.adapter",
                        "unisphere.source.path",
                        "unisphere.source.offset",
                        "unisphere.source.kind",
                        "unisphere.profile.version",
                    ] {
                        if !attrs.iter().any(|attr| attr["key"] == key) {
                            return Err(format!("missing {key}"));
                        }
                    }
                    if attrs.iter().any(|attr| {
                        attr["key"]
                            .as_str()
                            .is_some_and(|key| key.starts_with("gen_ai.usage."))
                    }) {
                        return Err(
                            "unverified native counters promoted to standard usage totals".into(),
                        );
                    }
                    records.push(record.clone());
                }
            }
        }
    }
    Ok(records)
}

fn external_consumer(repo: &Path, scratch: &Path) -> ProofResult<PathBuf> {
    let project = scratch.join("collection-consumer");
    fs::create_dir_all(project.join("src")).map_err(|e| e.to_string())?;
    let mut manifest =
        include_str!("../../../fixtures/consumer/collection-Cargo.toml.template").to_owned();
    for (token, package) in [
        ("@SDK_PATH@", "sdk"),
        ("@LOADER_PATH@", "loader-jsonl"),
        ("@ADAPTER_PATH@", "adapter-claude"),
        ("@OUTPUT_PATH@", "output-otlp"),
    ] {
        let path = repo.join("crates").join(package);
        require_file(&path.join("Cargo.toml"))?;
        manifest = manifest.replace(
            token,
            &serde_json::to_string(path.to_str().ok_or("nonUTF8 explicit package path")?)
                .map_err(|e| e.to_string())?,
        );
    }
    fs::write(project.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    fs::write(
        project.join("src/main.rs"),
        include_bytes!("../../../fixtures/consumer/collection-main.rs"),
    )
    .map_err(|e| e.to_string())?;
    build(
        repo,
        &scratch.join("build"),
        &[
            OsStr::new("build"),
            OsStr::new("--manifest-path"),
            project.join("Cargo.toml").as_os_str(),
        ],
    )?;
    let binary = scratch.join("build/target/debug").join(format!(
        "unisphere-collection-consumer{}",
        std::env::consts::EXE_SUFFIX
    ));
    require_file(&binary)?;
    Ok(binary)
}

fn cli_args(input: &Path, include_content: bool) -> Vec<OsString> {
    let mut args = strings(&["sessions", "export", "--adapter", "claude-code", "--input"]);
    args.push(input.as_os_str().to_owned());
    args.extend(strings(&["--max-records", "1"]));
    if include_content {
        args.push("--include-content".into());
    }
    args
}

pub fn run(repo: &Path, scratch: &Path) -> ProofResult<()> {
    let sdk = external_consumer(repo, scratch)?;
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
    let cli = scratch
        .join("build/target/debug")
        .join(format!("unisphere{}", std::env::consts::EXE_SUFFIX));
    require_file(&cli)?;
    let input = scratch.join("session.jsonl");
    let mut bytes = CLAUDE_BASIC.to_vec();
    bytes.extend_from_slice(b" \r\n");
    bytes.extend_from_slice(CLAUDE_PARTS);
    fs::write(&input, &bytes).map_err(|e| e.to_string())?;
    for include_content in [false, true] {
        let mode = if include_content {
            "content"
        } else {
            "metadata"
        };
        let cli_output = run_product(
            &cli,
            &scratch.join(format!("cli-{mode}")),
            &cli_args(&input, include_content),
        )?;
        expect_status(&cli_output, 0, "collection CLI")?;
        let sdk_output = run_product(
            &sdk,
            &scratch.join(format!("sdk-{mode}")),
            &[input.as_os_str().to_owned(), mode.into(), "1".into()],
        )?;
        expect_status(&sdk_output, 0, "external collection SDK")?;
        let cli_records = decode_records(&cli_output.stdout)?;
        let sdk_records = decode_records(&sdk_output.stdout)?;
        if cli_records != sdk_records || cli_records.len() != 6 {
            return Err("SDK/CLI collection parity or physical-record retention failed".into());
        }
        let summary: Value = serde_json::from_slice(&cli_output.stderr)
            .map_err(|e| format!("CLI summary invalid: {e}"))?;
        if summary["data"]["records"] != 6 || summary["data"]["incomplete_tail"] != false {
            return Err("wrong complete collection summary".into());
        }
        let text = String::from_utf8_lossy(&cli_output.stdout);
        if !include_content
            && (text.contains("SENSITIVE-")
                || cli_records
                    .iter()
                    .any(|record| record.get("body").is_some()))
        {
            return Err("metadata-only collection leaked fixture content".into());
        }
        if include_content
            && (!text.contains("SENSITIVE-USER-CONTENT")
                || !text.contains("SENSITIVE-REASONING")
                || !text.contains("SENSITIVE-TOOL-RESULT"))
        {
            return Err("opt-in collection lost supported structured content".into());
        }
        let mut hostile = sealed_command(&sdk, &scratch.join(format!("hostile-{mode}")))
            .map_err(|e| e.to_string())?;
        hostile
            .args([input.as_os_str(), OsStr::new(mode), OsStr::new("1")])
            .env("HOME", scratch.join("nonexistent-hostile-home"))
            .env(
                "XDG_CONFIG_HOME",
                scratch.join("nonexistent-hostile-config"),
            )
            .env("UNISPHERE_CONFIG", "/do/not/read/private.json")
            .env("CLAUDE_CONFIG_DIR", "/do/not/read/claude");
        let hostile_output = capture(&mut hostile)?;
        expect_status(&hostile_output, 0, "hostile-environment collection SDK")?;
        if hostile_output.stdout != sdk_output.stdout {
            return Err("ambient settings changed explicit SDK collection".into());
        }
    }
    let partial = scratch.join("partial.jsonl");
    let mut partial_bytes = CLAUDE_BASIC.to_vec();
    partial_bytes.extend_from_slice(b"{\"type\":");
    fs::write(&partial, &partial_bytes).map_err(|e| e.to_string())?;
    let result = run_product(
        &cli,
        &scratch.join("partial-runtime"),
        &cli_args(&partial, false),
    )?;
    expect_status(&result, 0, "partial tail")?;
    if decode_records(&result.stdout)?.len() != 3
        || serde_json::from_slice::<Value>(&result.stderr).map_err(|e| e.to_string())?["data"]["incomplete_tail"]
            != true
    {
        return Err("partial tail was skipped or misreported".into());
    }
    let output_file = scratch.join("keep-output.jsonl");
    fs::write(&output_file, b"keep-existing").map_err(|e| e.to_string())?;
    let mut args = cli_args(&input, false);
    args.push("--output".into());
    args.push(output_file.as_os_str().to_owned());
    let rejected = run_product(&cli, &scratch.join("existing-file"), &args)?;
    expect_status(&rejected, 1, "existing output rejection")?;
    if fs::read(&output_file).map_err(|e| e.to_string())? != b"keep-existing" {
        return Err("existing output overwritten".into());
    }
    let invalid = scratch.join("malformed.jsonl");
    fs::write(&invalid, b"{SENSITIVE-MALFORMED\n").map_err(|e| e.to_string())?;
    let invalid_output = run_product(
        &cli,
        &scratch.join("invalid-runtime"),
        &cli_args(&invalid, false),
    )?;
    expect_status(&invalid_output, 1, "malformed input")?;
    if !invalid_output.stdout.is_empty()
        || String::from_utf8_lossy(&invalid_output.stderr).contains("SENSITIVE-")
    {
        return Err("malformed input leaked or reached telemetry stdout".into());
    }
    // Install and execute the actual binary outside the repository with empty PATH.
    let install_root = scratch.join("installed");
    build(
        repo,
        &scratch.join("install-build"),
        &[
            OsStr::new("install"),
            OsStr::new("--locked"),
            OsStr::new("--path"),
            repo.join("crates/app").as_os_str(),
            OsStr::new("--root"),
            install_root.as_os_str(),
        ],
    )?;
    let installed = install_root
        .join("bin")
        .join(format!("unisphere{}", std::env::consts::EXE_SUFFIX));
    let installed_output = run_product(
        &installed,
        &scratch.join("installed-runtime"),
        &cli_args(&input, true),
    )?;
    expect_status(&installed_output, 0, "installed collection CLI")?;
    if decode_records(&installed_output.stdout)?.len() != 6 {
        return Err("installed CLI lost collection records".into());
    }
    let list = run_product(
        &installed,
        &scratch.join("list-runtime"),
        &[
            OsString::from("sessions"),
            "list".into(),
            "--root".into(),
            scratch.as_os_str().to_owned(),
        ],
    )?;
    expect_status(&list, 0, "installed list")?;
    let listing: Value = serde_json::from_slice(&list.stdout).map_err(|e| e.to_string())?;
    if listing["data"]["recursive"] != false
        || !listing["data"]["sessions"]
            .as_array()
            .ok_or("no listed sessions")?
            .iter()
            .any(|s| s["path"].as_str() == input.to_str())
    {
        return Err("explicit nonrecursive session listing lost input".into());
    }
    println!(
        "collection proof: pure adapter, SDK/CLI parity, metadata policy, partial tail, error isolation and installed runtime passed"
    );
    Ok(())
}
