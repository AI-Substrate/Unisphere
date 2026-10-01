#[cfg(unix)]
mod unix {
    use super::super::{
        ProofResult, build, capture, expect_status, require_file, run_product, strings,
    };
    use serde_json::{Value, json};
    use std::{
        collections::BTreeMap,
        ffi::{OsStr, OsString},
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::{Path, PathBuf},
    };
    use unisphere_testkit::{
        git_notes::{git_ok, initialize, standard_git},
        sealed_command,
    };

    const MIXED: &[u8] = include_bytes!("../../../../adapter-git-ai/fixtures/mixed.notes");
    fn external_consumer(repo: &Path, scratch: &Path) -> ProofResult<PathBuf> {
        let project = scratch.join("git-notes-consumer");
        fs::create_dir_all(project.join("src")).map_err(|e| e.to_string())?;
        let mut manifest =
            include_str!("../../../fixtures/consumer/git-notes-Cargo.toml.template").to_owned();
        for (token, package) in [
            ("@SDK_PATH@", "sdk"),
            ("@LOADER_PATH@", "loader-git"),
            ("@ADAPTER_PATH@", "adapter-git-ai"),
            ("@OUTPUT_PATH@", "output-otlp"),
        ] {
            let path = repo.join("crates").join(package);
            require_file(&path.join("Cargo.toml"))?;
            manifest = manifest.replace(
                token,
                &serde_json::to_string(path.to_str().ok_or("non-UTF8 dependency path")?)
                    .map_err(|e| e.to_string())?,
            );
        }
        fs::write(project.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
        fs::write(
            project.join("src/main.rs"),
            include_bytes!("../../../fixtures/consumer/git-notes-main.rs"),
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
        let binary = scratch.join("build/target/debug/unisphere-git-notes-consumer");
        require_file(&binary)?;
        Ok(binary)
    }
    fn args(source: &Path, git: &Path, command: &str) -> Vec<OsString> {
        let mut args = strings(&["sessions", command, "--adapter", "git-ai", "--repo"]);
        args.push(source.into());
        args.push("--git-executable".into());
        args.push(git.into());
        args
    }
    fn sdk_args(source: &Path, git: &Path, mode: &str, settings: &Value) -> Vec<OsString> {
        vec![
            mode.into(),
            source.into(),
            git.into(),
            settings.to_string().into(),
        ]
    }
    fn attribute<'a>(record: &'a Value, key: &str) -> Option<&'a Value> {
        record["attributes"]
            .as_array()?
            .iter()
            .find(|a| a["key"] == key)
            .map(|a| &a["value"])
    }
    fn records(bytes: &[u8]) -> ProofResult<Vec<Value>> {
        if bytes.last() != Some(&b'\n') {
            return Err("Git Notes OTLP lacks final LF".into());
        }
        let doc: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if doc
            .as_object()
            .is_none_or(|object| object.len() != 1 || !object.contains_key("resourceLogs"))
        {
            return Err("not an OTLP LogsData envelope".into());
        }
        let records = doc["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
            .as_array()
            .ok_or("missing Git Notes OTLP records")?
            .clone();
        for record in &records {
            if record.get("timeUnixNano").is_some()
                || record.get("spanId").is_some()
                || record.get("traceId").is_some()
                || attribute(record, "unisphere.source.offset").is_some()
            {
                return Err("Git Notes invented timing/span/offset".into());
            }
            for key in [
                "unisphere.source.path",
                "unisphere.source.kind",
                "unisphere.source.key",
                "unisphere.source.revision",
                "unisphere.git.repository.id",
                "unisphere.git.notes.ref",
                "unisphere.git.notes.tip",
            ] {
                if attribute(record, key).is_none() {
                    return Err(format!("missing common Git provenance {key}"));
                }
            }
            if attribute(record, "unisphere.profile.version").and_then(|v| v["intValue"].as_str())
                != Some("1")
                || attribute(record, "unisphere.source.adapter")
                    .and_then(|v| v["stringValue"].as_str())
                    != Some("git-ai")
                || attribute(record, "unisphere.source.format")
                    .and_then(|v| v["stringValue"].as_str())
                    != Some("git_notes")
            {
                return Err("wrong Git Notes profile/adapter/format".into());
            }
            if record["eventName"] != "unisphere.git_notes.snapshot" {
                for key in ["unisphere.git.commit", "unisphere.git.note.blob"] {
                    if attribute(record, key).is_none() {
                        return Err(format!("missing note provenance {key}"));
                    }
                }
            }
        }
        let last = records.last().ok_or("empty Git Notes envelope")?;
        if last["eventName"] != "unisphere.git_notes.snapshot"
            || attribute(last, "unisphere.git_notes.finality")
                .and_then(|v| v["stringValue"].as_str())
                != Some("unknown")
            || attribute(last, "unisphere.git_notes.semantics")
                .and_then(|v| v["stringValue"].as_str())
                != Some("replace_projection")
            || attribute(last, "unisphere.git_notes.records")
                .and_then(|v| v["intValue"].as_str())
                .and_then(|n| n.parse::<usize>().ok())
                != Some(records.len() - 1)
        {
            return Err("incorrect closing selection manifest".into());
        }
        for key in [
            "unisphere.git_notes.selection",
            "unisphere.git_notes.include_content",
            "unisphere.git_notes.ref_state",
            "unisphere.git_notes.notes",
        ] {
            if attribute(last, key).is_none() {
                return Err(format!("missing manifest field {key}"));
            }
        }
        Ok(records)
    }
    fn snapshot(root: &Path) -> ProofResult<BTreeMap<PathBuf, Vec<u8>>> {
        fn visit(path: &Path, values: &mut BTreeMap<PathBuf, Vec<u8>>) -> ProofResult<()> {
            for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                let kind = entry.file_type().map_err(|e| e.to_string())?;
                if kind.is_dir() {
                    visit(&path, values)?;
                } else if kind.is_symlink() {
                    values.insert(
                        path.clone(),
                        fs::read_link(&path)
                            .map_err(|e| e.to_string())?
                            .as_os_str()
                            .as_encoded_bytes()
                            .to_vec(),
                    );
                } else {
                    values.insert(path.clone(), fs::read(&path).map_err(|e| e.to_string())?);
                }
            }
            Ok(())
        }
        let mut values = BTreeMap::new();
        visit(root, &mut values)?;
        Ok(values)
    }
    fn cli_failure(
        cli: &Path,
        runtime: &Path,
        args: &[OsString],
        code: i32,
        kind: &str,
    ) -> ProofResult<()> {
        let result = run_product(cli, runtime, args)?;
        expect_status(&result, code, "Git Notes expected CLI failure")?;
        let error: Value = serde_json::from_slice(&result.stderr).map_err(|e| e.to_string())?;
        if !result.stdout.is_empty()
            || error["error"]["kind"] != kind
            || String::from_utf8_lossy(&result.stderr).contains("SENSITIVE")
        {
            return Err(format!("unsafe/incorrect Git Notes failure: {error}"));
        }
        Ok(())
    }
    fn parity(
        cli: &Path,
        sdk: &Path,
        runtime: &Path,
        source: &Path,
        git: &Path,
        content: bool,
    ) -> ProofResult<Vec<Value>> {
        let before = snapshot(source)?;
        let mut cli_args = args(source, git, "export");
        if content {
            cli_args.push("--include-content".into());
        }
        let cli_result = run_product(cli, &runtime.join("cli"), &cli_args)?;
        let sdk_result = run_product(
            sdk,
            &runtime.join("sdk"),
            &sdk_args(
                source,
                git,
                if content { "content" } else { "export" },
                &json!({}),
            ),
        )?;
        expect_status(
            &cli_result,
            0,
            &format!("Git Notes installed CLI at {}", source.display()),
        )?;
        expect_status(
            &sdk_result,
            0,
            &format!("Git Notes external SDK at {}", source.display()),
        )?;
        if cli_result.stdout != sdk_result.stdout {
            return Err("Git Notes SDK/installed CLI bytes diverged".into());
        }
        let decoded = records(&cli_result.stdout)?;
        if String::from_utf8_lossy(&cli_result.stdout).contains("SENSITIVE") != content {
            return Err("Git Notes content-policy failure".into());
        }
        if snapshot(source)? != before {
            return Err("ingestion changed source refs/index/worktree/config/hooks/objects".into());
        }
        Ok(decoded)
    }
    pub fn run(repo: &Path, scratch: &Path) -> ProofResult<()> {
        let git = standard_git()?;
        let sdk = external_consumer(repo, scratch)?;
        let install = scratch.join("install");
        build(
            repo,
            &scratch.join("build"),
            &[
                OsStr::new("install"),
                OsStr::new("--path"),
                repo.join("crates/app").as_os_str(),
                OsStr::new("--root"),
                install.as_os_str(),
                OsStr::new("--debug"),
                OsStr::new("--locked"),
            ],
        )?;
        let cli = install.join("bin/unisphere");
        require_file(&cli)?;
        let source = scratch.join("repository with ; literal spaces");
        let commit = initialize(&source, &git, false, MIXED)?;
        let runtime = scratch.join("runtime");
        let metadata = parity(&cli, &sdk, &runtime.join("metadata"), &source, &git, false)?;
        parity(&cli, &sdk, &runtime.join("content"), &source, &git, true)?;
        if !metadata.iter().any(|r| {
            attribute(r, "unisphere.git_ai.identity_resolution")
                .is_some_and(|v| v["stringValue"] == "unresolved")
        }) || !metadata.iter().any(|r| {
            attribute(r, "unisphere.git_ai.attestation.key")
                .is_some_and(|v| v["stringValue"] == "abc1234")
        }) || !metadata.iter().any(|r| {
            attribute(r, "unisphere.git_ai.agent.id")
                .is_some_and(|v| v["stringValue"] == "synthetic-unreferenced")
        }) {
            return Err("mixed/unresolved/seven-char/unreferenced evidence was lost".into());
        }
        let listed = run_product(
            &sdk,
            &runtime.join("list-sdk"),
            &sdk_args(&source, &git, "list", &json!({})),
        )?;
        expect_status(&listed, 0, "external SDK list")?;
        let listing: Value = serde_json::from_slice(&listed.stdout).map_err(|e| e.to_string())?;
        let cli_listed = run_product(
            &cli,
            &runtime.join("list-cli"),
            &args(&source, &git, "list"),
        )?;
        expect_status(&cli_listed, 0, "installed CLI list")?;
        let cli_listing: Value =
            serde_json::from_slice(&cli_listed.stdout).map_err(|e| e.to_string())?;
        if cli_listing["data"]["listing"] != listing
            || listing["notes"][0]["target_commit"] != commit
        {
            return Err("public SDK/CLI listing/provenance mismatch".into());
        }
        super::super::git_query::run(&cli, &source, &git, &runtime.join("query"))?;
        let pinned = listing["notes"][0].clone();
        let original = run_product(
            &sdk,
            &runtime.join("read-original"),
            &sdk_args(&source, &git, "read", &pinned),
        )?;
        expect_status(&original, 0, "external pinned read")?;
        git_ok(
            &git,
            &source,
            &[
                "notes",
                "--ref=refs/notes/ai",
                "add",
                "-f",
                "-F",
                "-",
                &commit,
            ],
            Some(b"changed non-format note"),
        )?;
        let retained = run_product(
            &sdk,
            &runtime.join("read-retained"),
            &sdk_args(&source, &git, "read", &pinned),
        )?;
        expect_status(&retained, 0, "retained pinned read")?;
        if retained.stdout != original.stdout {
            return Err("moving ref changed an immutable pinned read".into());
        }
        let mut forged = pinned.clone();
        forged["note_blob"] = json!("f".repeat(40));
        let forged_result = run_product(
            &sdk,
            &runtime.join("forged"),
            &sdk_args(&source, &git, "read", &forged),
        )?;
        expect_status(&forged_result, 1, "forged pinned reference")?;
        if serde_json::from_slice::<Value>(&forged_result.stderr).map_err(|e| e.to_string())?["kind"]
            != "invalid_data"
        {
            return Err("forged blob was not rejected".into());
        }
        git_ok(
            &git,
            &source,
            &[
                "notes",
                "--ref=refs/notes/ai",
                "add",
                "-f",
                "-F",
                "-",
                &commit,
            ],
            Some(MIXED),
        )?;
        let before_tracking = run_product(
            &cli,
            &runtime.join("tracking-before"),
            &args(&source, &git, "export"),
        )?;
        let tip = String::from_utf8(git_ok(
            &git,
            &source,
            &["rev-parse", "refs/notes/ai"],
            None,
        )?)
        .map_err(|e| e.to_string())?;
        git_ok(
            &git,
            &source,
            &["update-ref", "refs/notes/ai-tracking", tip.trim()],
            None,
        )?;
        let after_tracking = run_product(
            &cli,
            &runtime.join("tracking-after"),
            &args(&source, &git, "export"),
        )?;
        expect_status(&before_tracking, 0, "canonical before tracking")?;
        expect_status(&after_tracking, 0, "canonical after tracking")?;
        if before_tracking.stdout != after_tracking.stdout {
            return Err("unselected tracking ref changed canonical observations".into());
        }
        let mut explicit = args(&source, &git, "export");
        explicit.extend(strings(&["--commit", &commit, "--commit", &commit]));
        let selected = run_product(&cli, &runtime.join("selected"), &explicit)?;
        expect_status(&selected, 0, "deduplicated explicit selection")?;
        if records(&selected.stdout)?.len() != metadata.len() {
            return Err("duplicate commit selector duplicated attribution".into());
        }
        for (name, settings, ref_state) in [
            ("empty-selection", json!({"commits":[]}), "present"),
            (
                "missing-ref",
                json!({"notes_ref":"refs/notes/missing"}),
                "missing",
            ),
        ] {
            let empty = run_product(
                &sdk,
                &runtime.join(name),
                &sdk_args(&source, &git, "export", &settings),
            )?;
            expect_status(&empty, 0, name)?;
            let decoded = records(&empty.stdout)?;
            if decoded.len() != 1
                || attribute(&decoded[0], "unisphere.git_notes.ref_state")
                    .is_none_or(|v| v["stringValue"] != ref_state)
            {
                return Err("empty and absent-ref outcomes are conflated".into());
            }
        }
        let mut missing_cli = args(&source, &git, "export");
        missing_cli.extend(strings(&["--notes-ref", "refs/notes/missing"]));
        let empty = run_product(&cli, &runtime.join("missing-cli"), &missing_cli)?;
        expect_status(&empty, 0, "missing notes without Git AI")?;
        if records(&empty.stdout)?.len() != 1 {
            return Err("missing notes not represented by manifest".into());
        }
        for mode in ["write-failure", "flush-failure"] {
            let broken = run_product(
                &sdk,
                &runtime.join(mode),
                &sdk_args(&source, &git, mode, &json!({})),
            )?;
            expect_status(&broken, 1, "SDK failed output")?;
            if serde_json::from_slice::<Value>(&broken.stderr).map_err(|e| e.to_string())?["output_code"]
                != "UNI-WRITE"
            {
                return Err(format!("{mode} was accepted"));
            }
        }
        let mut bad_git = args(&source, &scratch.join("absent-git"), "export");
        cli_failure(
            &cli,
            &runtime.join("missing-git"),
            &bad_git,
            1,
            "git_unavailable",
        )?;
        bad_git.truncate(bad_git.len() - 2);
        cli_failure(
            &cli,
            &runtime.join("empty-path"),
            &bad_git,
            1,
            "git_unavailable",
        )?;
        let help = run_product(
            &cli,
            &runtime.join("help-no-git"),
            &strings(&["sessions", "export", "--adapter", "git-ai", "--help"]),
        )?;
        expect_status(&help, 0, "help without Git")?;
        for (name, flags, code, kind) in [
            (
                "invalid-ref",
                vec!["--notes-ref", "refs/notes/../bad"],
                2,
                "invalid_input",
            ),
            (
                "wrong-namespace",
                vec!["--notes-ref", "refs/not-notes/retired"],
                2,
                "invalid_input",
            ),
            (
                "shell-argument",
                vec!["--commit", "$(touch should-not-run)"],
                2,
                "invalid_input",
            ),
            ("note-limit", vec!["--max-note-bytes", "2"], 1, "note_limit"),
            (
                "record-limit",
                vec!["--max-records", "1"],
                1,
                "record_limit",
            ),
        ] {
            let mut invocation = args(&source, &git, "export");
            invocation.extend(strings(&flags));
            cli_failure(&cli, &runtime.join(name), &invocation, code, kind)?;
        }
        let before = snapshot(&source)?;
        let alias = scratch.join("git-dir-alias");
        symlink(source.join(".git"), &alias).map_err(|e| e.to_string())?;
        for (index, output) in [
            source.join("forbidden.jsonl"),
            source.join(".git/forbidden.jsonl"),
            alias.join("forbidden.jsonl"),
        ]
        .iter()
        .enumerate()
        {
            let mut invocation = args(&source, &git, "export");
            invocation.push("--output".into());
            invocation.push(output.into());
            cli_failure(
                &cli,
                &runtime.join(format!("output-guard-{index}")),
                &invocation,
                2,
                "invalid_input",
            )?;
        }
        if snapshot(&source)? != before {
            return Err("output guard modified the source".into());
        }
        let output_file = scratch.join("accepted.jsonl");
        let mut destination = args(&source, &git, "export");
        destination.push("--output".into());
        destination.push(output_file.clone().into_os_string());
        let saved = run_product(&cli, &runtime.join("output-file"), &destination)?;
        expect_status(&saved, 0, "explicit external output")?;
        records(&fs::read(&output_file).map_err(|e| e.to_string())?)?;
        cli_failure(
            &cli,
            &runtime.join("no-overwrite"),
            &destination,
            1,
            "output",
        )?;
        let mut hostile =
            sealed_command(&cli, &runtime.join("hostile")).map_err(|e| e.to_string())?;
        hostile
            .args(args(&source, &git, "export"))
            .env("GIT_DIR", scratch.join("not-a-repository"))
            .env("GIT_WORK_TREE", scratch.join("wrong-worktree"))
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "remote.evil.promisor")
            .env("GIT_CONFIG_VALUE_0", "true")
            .env("GIT_CONFIG_GLOBAL", scratch.join("not-config"))
            .env("GIT_SSH_COMMAND", "touch SENSITIVE-should-not-run");
        let hostile_result = capture(&mut hostile)?;
        expect_status(&hostile_result, 0, "hostile inherited Git environment")?;
        if hostile_result.stdout != after_tracking.stdout {
            return Err("inherited Git environment influenced export".into());
        }
        let bare = scratch.join("bare");
        initialize(&bare, &git, true, MIXED)?;
        parity(&cli, &sdk, &runtime.join("bare"), &bare, &git, false)?;
        let linked = scratch.join("linked");
        git_ok(
            &git,
            &source,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().ok_or("nonUTF8 linked path")?,
                "HEAD",
            ],
            None,
        )?;
        let nested = linked.join("nested");
        fs::create_dir(&nested).map_err(|e| e.to_string())?;
        let linked_before = (snapshot(&source)?, snapshot(&linked)?);
        parity(&cli, &sdk, &runtime.join("linked"), &nested, &git, false)?;
        let mut linked_output = args(&nested, &git, "export");
        linked_output.push("--output".into());
        linked_output.push(linked.join("forbidden.jsonl").into_os_string());
        cli_failure(
            &cli,
            &runtime.join("linked-output"),
            &linked_output,
            2,
            "invalid_input",
        )?;
        if linked_before != (snapshot(&source)?, snapshot(&linked)?) {
            return Err("linked/nested ingestion or output guard changed source storage".into());
        }
        for (name, payload, kind) in [
            ("malformed", b"SENSITIVE-malformed".to_vec(), "invalid_data"),
            (
                "unsupported",
                String::from_utf8_lossy(MIXED)
                    .replace("authorship/3.0.0", "authorship/4.0.0")
                    .into_bytes(),
                "unsupported_format",
            ),
        ] {
            let malformed = scratch.join(name);
            initialize(&malformed, &git, true, &payload)?;
            cli_failure(
                &cli,
                &runtime.join(name),
                &args(&malformed, &git, "export"),
                1,
                kind,
            )?;
        }
        let total_source = scratch.join("two-notes");
        initialize(&total_source, &git, true, MIXED)?;
        let tree = String::from_utf8(git_ok(
            &git,
            &total_source,
            &["rev-parse", "HEAD^{tree}"],
            None,
        )?)
        .map_err(|e| e.to_string())?;
        let second = String::from_utf8(git_ok(
            &git,
            &total_source,
            &["commit-tree", tree.trim()],
            Some(b"Second synthetic commit\n"),
        )?)
        .map_err(|e| e.to_string())?;
        git_ok(
            &git,
            &total_source,
            &[
                "notes",
                "--ref=refs/notes/ai",
                "add",
                "-F",
                "-",
                second.trim(),
            ],
            Some(MIXED),
        )?;
        let mut total_args = args(&total_source, &git, "export");
        total_args.extend(strings(&[
            "--max-note-bytes",
            "1500",
            "--max-total-bytes",
            "2000",
        ]));
        cli_failure(
            &cli,
            &runtime.join("total-bytes"),
            &total_args,
            1,
            "batch_limit",
        )?;
        let total_sdk = run_product(
            &sdk,
            &runtime.join("total-sdk"),
            &sdk_args(
                &total_source,
                &git,
                "export",
                &json!({"max_note_bytes":1500,"max_total_bytes":2000}),
            ),
        )?;
        expect_status(&total_sdk, 1, "external SDK total-byte bound")?;
        if !total_sdk.stdout.is_empty()
            || serde_json::from_slice::<Value>(&total_sdk.stderr).map_err(|e| e.to_string())?["kind"]
                != "batch_limit"
        {
            return Err("SDK silently accepted partial total-byte selection".into());
        }
        let note_blob = String::from_utf8(git_ok(
            &git,
            &total_source,
            &["hash-object", "-w", "--stdin"],
            Some(MIXED),
        )?)
        .map_err(|e| e.to_string())?;
        let missing_tree = String::from_utf8(git_ok(
            &git,
            &total_source,
            &["mktree"],
            Some(format!("100644 blob {}\t{}\n", note_blob.trim(), "0".repeat(40)).as_bytes()),
        )?)
        .map_err(|e| e.to_string())?;
        let missing_tip = String::from_utf8(git_ok(
            &git,
            &total_source,
            &["commit-tree", missing_tree.trim()],
            Some(b"Missing target fixture\n"),
        )?)
        .map_err(|e| e.to_string())?;
        git_ok(
            &git,
            &total_source,
            &[
                "update-ref",
                "refs/notes/missing-target",
                missing_tip.trim(),
            ],
            None,
        )?;
        let mut missing_target = args(&total_source, &git, "export");
        missing_target.extend(strings(&["--notes-ref", "refs/notes/missing-target"]));
        cli_failure(
            &cli,
            &runtime.join("missing-object"),
            &missing_target,
            1,
            "object_read",
        )?;
        git_ok(
            &git,
            &total_source,
            &["update-ref", "refs/notes/not-commit", tree.trim()],
            None,
        )?;
        let mut invalid_tip = args(&total_source, &git, "export");
        invalid_tip.extend(strings(&["--notes-ref", "refs/notes/not-commit"]));
        cli_failure(
            &cli,
            &runtime.join("invalid-tip"),
            &invalid_tip,
            1,
            "invalid_ref",
        )?;
        let expanded = scratch.join("encoded-expansion");
        let ranges = (1..=60)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let expanded_note = format!(
            "\"{}\"\n  abc1234 {ranges}\n---\n{{\"schema_version\":\"authorship/3.0.0\"}}\n",
            "\\u0001".repeat(100_000)
        );
        initialize(&expanded, &git, true, expanded_note.as_bytes())?;
        let overflow = run_product(
            &cli,
            &runtime.join("encoded-limit"),
            &args(&expanded, &git, "export"),
        )?;
        expect_status(&overflow, 1, "encoded output limit")?;
        if !overflow.stdout.is_empty()
            || serde_json::from_slice::<Value>(&overflow.stderr).map_err(|e| e.to_string())?["error"]
                ["output_code"]
                != "UNI-LIMIT-OUTPUT"
        {
            return Err("encoded expansion was not rejected before output".into());
        }
        let blob = String::from_utf8(git_ok(
            &git,
            &source,
            &["rev-parse", "HEAD:source.rs"],
            None,
        )?)
        .map_err(|e| e.to_string())?;
        git_ok(
            &git,
            &source,
            &[
                "notes",
                "--ref=refs/notes/ai",
                "add",
                "-F",
                "-",
                blob.trim(),
            ],
            Some(MIXED),
        )?;
        cli_failure(
            &cli,
            &runtime.join("noncommit"),
            &args(&source, &git, "export"),
            1,
            "unsupported_target",
        )?;
        let selected_ok = run_product(&cli, &runtime.join("ignore-unselected-target"), &explicit)?;
        expect_status(
            &selected_ok,
            0,
            "explicit commit excludes unrelated unsupported targets",
        )?;
        let target_tree =
            String::from_utf8(git_ok(&git, &source, &["rev-parse", "HEAD^{tree}"], None)?)
                .map_err(|e| e.to_string())?;
        git_ok(
            &git,
            &source,
            &[
                "notes",
                "--ref=refs/notes/ai",
                "add",
                "-F",
                "-",
                target_tree.trim(),
            ],
            Some(MIXED),
        )?;
        let mut small_all = args(&source, &git, "export");
        small_all.extend(strings(&["--max-listing-bytes", "180"]));
        cli_failure(
            &cli,
            &runtime.join("all-listing-limit"),
            &small_all,
            1,
            "listing_limit",
        )?;
        let mut small_selected = explicit.clone();
        small_selected.extend(strings(&["--max-listing-bytes", "180"]));
        let bounded_selection = run_product(
            &cli,
            &runtime.join("direct-bounded-selection"),
            &small_selected,
        )?;
        expect_status(
            &bounded_selection,
            0,
            "direct selection avoids unrelated listing volume",
        )?;
        let bounded_sdk = run_product(
            &sdk,
            &runtime.join("direct-bounded-sdk"),
            &sdk_args(
                &source,
                &git,
                "export",
                &json!({"commits":[commit],"max_listing_bytes":180}),
            ),
        )?;
        expect_status(&bounded_sdk, 0, "external SDK bounded direct selection")?;
        if bounded_selection.stdout != bounded_sdk.stdout {
            return Err("bounded direct SDK/CLI selection diverged".into());
        }
        git_ok(
            &git,
            &source,
            &["config", "remote.evil.promisor", "true"],
            None,
        )?;
        cli_failure(
            &cli,
            &runtime.join("promisor"),
            &args(&source, &git, "export"),
            1,
            "unsupported_repository",
        )?;
        // This negative-only executable forces standard Git's real ownership check; positive proof never uses a wrapper.
        let foreign = scratch.join("foreign-git");
        fs::write(
            &foreign,
            format!(
                "#!/bin/sh\nGIT_TEST_ASSUME_DIFFERENT_OWNER=1 exec {} \"$@\"\n",
                git.display()
            ),
        )
        .map_err(|e| e.to_string())?;
        fs::set_permissions(&foreign, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        cli_failure(
            &cli,
            &runtime.join("ownership"),
            &args(&bare, &foreign, "export"),
            1,
            "unsafe_repository",
        )?;
        println!(
            "Git Notes: real SDK + installed CLI parity; mixed/unresolved/7-hex privacy; pinned reads; explicit selection; empty/absent; bare/worktree; source/output safety; missing Git, unsupported/malformed/bounded/output/ownership/promisor failures. Positive runtimes use empty PATH and explicit standard Git, never Git AI."
        );
        Ok(())
    }
}
#[cfg(unix)]
pub use unix::run;
#[cfg(not(unix))]
pub fn run(_repo: &std::path::Path, _scratch: &std::path::Path) -> super::ProofResult<()> {
    Err("Git Notes runtime proof requires Unix".into())
}
