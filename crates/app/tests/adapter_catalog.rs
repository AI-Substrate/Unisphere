use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    process::{Command, Output},
};

fn catalog(cwd: &Path, home: &Path, config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_unisphere"))
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("CLAUDE_CONFIG_DIR", config)
        .env("XDG_CONFIG_HOME", config)
        .env("UNISPHERE_CONFIG", config)
        .args(args)
        .output()
        .unwrap()
}

fn descriptor(output: &Output) -> serde_json::Value {
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["ok"], true);
    assert_eq!(document["command"], "adapters.list");
    assert_eq!(document["v"], 1);
    let adapters = document["data"]["adapters"].as_array().unwrap();
    let ids: BTreeSet<_> = adapters
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "claude-code",
            "codex",
            "oh-my-pi",
            "pi",
            "copilot-cli",
            "copilot-cli-snapshot",
            "cursor-transcript",
            "cursor-ide",
            "vscode-copilot",
        ])
    );
    for adapter in adapters {
        assert_eq!(
            adapter["capabilities"]["delayed_revision_reconciliation"],
            false
        );
        for location in adapter["locations"].as_array().unwrap() {
            assert!(matches!(
                location["base"].as_str(),
                Some("home" | "appdata")
            ));
            assert!(matches!(
                location["storage_format"].as_str(),
                Some("jsonl" | "json_document" | "json_journal" | "sqlite_key_value")
            ));
            if adapter["id"] == "vscode-copilot" {
                let suffix = if location["storage_format"] == "json_document" {
                    "*.json"
                } else {
                    "*.jsonl"
                };
                assert!(location["session_glob"].as_str().unwrap().ends_with(suffix));
            }
        }
    }
    adapters
        .iter()
        .find(|entry| entry["id"] == "claude-code")
        .unwrap()
        .clone()
}

#[test]
fn catalog_json_is_static_under_hostile_environment() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first-home");
    let second = root.path().join("second-home");
    fs::create_dir_all(first.join(".claude/projects/synthetic-project")).unwrap();
    fs::create_dir_all(second.join(".claude/projects/different-project")).unwrap();
    fs::write(
        first.join(".claude/projects/synthetic-project/session.jsonl"),
        b"SYNTHETIC-PRIVATE-MARKER\n",
    )
    .unwrap();
    let initial = catalog(root.path(), &first, &first, &["adapters", "list", "--json"]);
    let changed = catalog(
        root.path(),
        &second,
        &second,
        &["adapters", "list", "--json"],
    );
    let entry = descriptor(&initial);
    descriptor(&changed);
    assert_eq!(changed.stdout, initial.stdout);
    assert_eq!(entry["application"], "Claude Code");
    assert_eq!(entry["locations"][0]["base"], "home");
    assert_eq!(entry["locations"][0]["path"], ".claude/projects");
    assert_eq!(entry["locations"][0]["session_glob"], "*/*.jsonl");
    assert_eq!(entry["locations"][0]["storage_format"], "jsonl");
    assert_eq!(
        entry["capabilities"]["export_platforms"],
        serde_json::json!(["unix"])
    );
    assert_eq!(
        entry["capabilities"]["output_formats"],
        serde_json::json!(["otlp-jsonl"])
    );
    assert_eq!(entry["capabilities"]["sdk_caller_owned_cursor"], true);
    assert_eq!(
        entry["capabilities"]["cursor_source_assumption"],
        "append_only"
    );
    assert_eq!(entry["capabilities"]["cli_persisted_resume"], false);
    assert_eq!(
        entry["capabilities"]["delayed_revision_reconciliation"],
        false
    );
    assert_eq!(entry["capabilities"]["lossless_archive"], false);
    assert!(
        !String::from_utf8(initial.stdout)
            .unwrap()
            .contains("SYNTHETIC-PRIVATE-MARKER")
    );
}

#[test]
fn catalog_does_not_resolve_inaccessible_store_roots() {
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("not-a-directory");
    fs::write(&blocked, b"SYNTHETIC-INVALID-CONFIG").unwrap();
    // No user, including a privileged test runner, can traverse this as a home.
    assert!(fs::read_dir(blocked.join(".claude/projects")).is_err());
    let output = catalog(
        root.path(),
        &blocked,
        &blocked,
        &["adapters", "list", "--json"],
    );
    let entry = descriptor(&output);
    assert!(!Path::new(entry["locations"][0]["path"].as_str().unwrap()).is_absolute());
}

#[test]
fn catalog_argument_errors_hide_input_and_retain_machine_classification() {
    let root = tempfile::tempdir().unwrap();
    let output = catalog(
        root.path(),
        root.path(),
        root.path(),
        &["adapters", "list", "--json", "--SYNTHETIC-PRIVATE-ARGUMENT"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["ok"], false);
    assert_eq!(document["v"], 1);
    assert_eq!(document["error"]["code"], "UNI-CLI-ARGUMENT");
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("SYNTHETIC-PRIVATE-ARGUMENT")
    );
}
