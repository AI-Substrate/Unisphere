#![forbid(unsafe_code)]

use serde_json::Value;
use std::{
    collections::BTreeSet,
    env, fs,
    path::PathBuf,
    process::{Command, ExitCode},
};

// Check declarations, not only resolved/active edges: optional and target-specific
// dependencies must obey the same boundary even on a machine that never builds them.
fn allowed(package: &str, dependency: &str, kind: &str) -> bool {
    match (package, kind) {
        ("unisphere-core", "normal") => {
            matches!(dependency, "serde" | "serde_json" | "sha2" | "time")
        }
        ("unisphere-sdk", "normal") => {
            matches!(
                dependency,
                "unisphere-core" | "serde" | "serde_json" | "regex" | "globset" | "sha2" | "time"
            )
        }
        ("unisphere-cli", "normal") => {
            matches!(dependency, "unisphere-core" | "clap" | "serde_json")
        }
        ("unisphere-app", "normal") => matches!(
            dependency,
            "unisphere-sdk"
                | "unisphere-cli"
                | "unisphere-loader-jsonl"
                | "unisphere-adapter-claude"
                | "unisphere-adapter-codex"
                | "unisphere-adapter-omp"
                | "unisphere-adapter-pi"
                | "unisphere-adapter-copilot-cli"
                | "unisphere-adapter-vscode-copilot"
                | "unisphere-adapter-cursor"
                | "unisphere-loader-snapshot"
                | "unisphere-output-otlp"
                | "unisphere-loader-query"
                | "unisphere-output-query"
        ),
        ("unisphere-loader-jsonl", "normal") => {
            matches!(dependency, "unisphere-core" | "libc" | "sha2")
        }
        ("unisphere-loader-snapshot", "normal") => matches!(
            dependency,
            "unisphere-core" | "libc" | "serde" | "serde_json" | "sha2" | "rusqlite"
        ),
        ("unisphere-loader-query", "normal") => matches!(
            dependency,
            "unisphere-core"
                | "unisphere-loader-jsonl"
                | "globset"
                | "unisphere-loader-snapshot"
                | "serde_json"
                | "sha2"
                | "libc"
        ),
        ("unisphere-output-query", "normal") => {
            matches!(dependency, "unisphere-core" | "serde" | "serde_json")
        }
        (
            "unisphere-adapter-claude"
            | "unisphere-adapter-codex"
            | "unisphere-adapter-omp"
            | "unisphere-adapter-pi"
            | "unisphere-adapter-copilot-cli"
            | "unisphere-adapter-vscode-copilot"
            | "unisphere-adapter-cursor",
            "normal",
        ) => {
            matches!(dependency, "unisphere-core" | "serde_json" | "time")
        }
        ("unisphere-output-otlp", "normal") => {
            matches!(dependency, "unisphere-core" | "serde_json")
        }
        ("unisphere-testkit", "normal") => {
            matches!(dependency, "unisphere-core" | "serde_json" | "tempfile")
        }
        ("unisphere-app", "dev") => {
            matches!(
                dependency,
                "unisphere-testkit" | "tempfile" | "serde_json" | "rusqlite"
            )
        }
        ("unisphere-cli", "dev") => matches!(
            dependency,
            "unisphere-testkit" | "unisphere-output-query" | "tempfile" | "serde_json"
        ),
        (
            "unisphere-sdk"
            | "unisphere-loader-jsonl"
            | "unisphere-adapter-claude"
            | "unisphere-adapter-codex"
            | "unisphere-adapter-omp"
            | "unisphere-adapter-pi"
            | "unisphere-adapter-copilot-cli"
            | "unisphere-adapter-vscode-copilot"
            | "unisphere-adapter-cursor"
            | "unisphere-loader-snapshot"
            | "unisphere-output-otlp"
            | "unisphere-loader-query"
            | "unisphere-output-query",
            "dev",
        ) => matches!(dependency, "unisphere-testkit" | "tempfile" | "serde_json"),
        _ => false,
    }
}

fn check(graph: &Value) -> Result<usize, String> {
    let packages = graph["packages"]
        .as_array()
        .ok_or("metadata lacks packages array")?;
    let members = graph["workspace_members"]
        .as_array()
        .ok_or("metadata lacks workspace_members array")?;
    if members.is_empty() {
        return Err("workspace contains no members".into());
    }
    let mut names = BTreeSet::new();
    let mut count = 0;
    for member in members {
        let id = member
            .as_str()
            .ok_or("workspace member id is not a string")?;
        let package = packages
            .iter()
            .find(|p| p["id"].as_str() == Some(id))
            .ok_or_else(|| format!("workspace member {id} has no package record"))?;
        let name = package["name"].as_str().ok_or("package lacks name")?;
        if !matches!(
            name,
            "unisphere-core"
                | "unisphere-sdk"
                | "unisphere-cli"
                | "unisphere-app"
                | "unisphere-testkit"
                | "unisphere-loader-jsonl"
                | "unisphere-adapter-claude"
                | "unisphere-adapter-codex"
                | "unisphere-adapter-omp"
                | "unisphere-adapter-pi"
                | "unisphere-adapter-copilot-cli"
                | "unisphere-adapter-vscode-copilot"
                | "unisphere-adapter-cursor"
                | "unisphere-loader-snapshot"
                | "unisphere-output-otlp"
                | "unisphere-loader-query"
                | "unisphere-output-query"
        ) {
            return Err(format!("unapproved workspace package {name}"));
        }
        if !names.insert(name) {
            return Err(format!("duplicate workspace package {name}"));
        }
        for dependency in package["dependencies"]
            .as_array()
            .ok_or("package lacks dependencies array")?
        {
            let target = dependency["name"]
                .as_str()
                .ok_or("dependency lacks package name")?;
            let kind = match dependency.get("kind") {
                Some(Value::Null) => "normal",
                Some(Value::String(kind))
                    if matches!(kind.as_str(), "normal" | "dev" | "build") =>
                {
                    kind
                }
                _ => {
                    return Err(format!(
                        "{name} -> {target}: missing or unknown dependency kind"
                    ));
                }
            };
            if !allowed(name, target, kind) {
                return Err(format!(
                    "forbidden {kind} dependency: {name} -> {target} (target={}, optional={}, rename={})",
                    dependency["target"], dependency["optional"], dependency["rename"]
                ));
            }
            // A renamed dependency still uses Cargo's actual package name above.
            // A registry/git substitute for an internal contract is never allowed.
            if target.starts_with("unisphere-") && !dependency["path"].is_string() {
                return Err(format!(
                    "{name} -> {target}: internal dependency must use a local path"
                ));
            }
            count += 1;
        }
    }
    for required in ["unisphere-core", "unisphere-testkit"] {
        if !names.contains(required) {
            return Err(format!("required baseline package {required} absent"));
        }
    }
    Ok(count)
}

// A bounded lexical sensor, not a Rust effect system: import aliases, macros and
// indirect calls still require independent source review. Conventional trailing
// #[cfg(test)] modules and doc/comment-only lines are outside production scope.
fn check_source(source: &str, label: &str) -> Result<(), String> {
    let production = source.split("\n#[cfg(test)]").next().unwrap_or(source);
    let text: String = production
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    for module in ["fs", "env", "process", "net", "thread"] {
        if compact.contains(&format!("std::{module}")) {
            return Err(format!(
                "{label}: forbidden production std::{module} access"
            ));
        }
        for statement in text.split(';') {
            let statement_compact: String =
                statement.chars().filter(|c| !c.is_whitespace()).collect();
            if statement_compact.contains("usestd::{")
                && statement
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .any(|token| token == module)
            {
                return Err(format!("{label}: forbidden grouped std::{module} import"));
            }
        }
    }
    let tokens: Vec<_> = text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.contains(&"unsafe")
        || tokens.contains(&"extern")
        || compact.contains("SystemTime::now")
        || compact.contains("Instant::now")
        || compact.contains("OffsetDateTime::now_utc")
        || compact.contains("include_str!")
        || compact.contains("include_bytes!")
    {
        return Err(format!(
            "{label}: forbidden production clock, unsafe/FFI or file-include access"
        ));
    }
    Ok(())
}

fn check_sources(directory: &std::path::Path) -> Result<usize, String> {
    if !directory.exists() {
        return Ok(0);
    }
    let mut checked = 0;
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "source scan refuses symlink: {}",
                entry.path().display()
            ));
        }
        if kind.is_dir() {
            checked += check_sources(&entry.path())?;
        } else if entry.path().extension().is_some_and(|ext| ext == "rs") {
            check_source(
                &fs::read_to_string(entry.path()).map_err(|e| e.to_string())?,
                &entry.path().display().to_string(),
            )?;
            checked += 1;
        }
    }
    Ok(checked)
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let argument = args.next();
    let real_workspace = argument.is_none();
    let graph = match argument.as_deref() {
        None => {
            let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
                .output()
                .map_err(|e| format!("cargo metadata could not start: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "cargo metadata failed: {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            output.stdout
        }
        Some(flag) if flag == "--metadata" => {
            let path = PathBuf::from(args.next().ok_or("--metadata requires a path")?);
            if args.next().is_some() {
                return Err("usage: unisphere-arch-check [--metadata FILE]".into());
            }
            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?
        }
        _ => return Err("usage: unisphere-arch-check [--metadata FILE]".into()),
    };
    let value =
        serde_json::from_slice(&graph).map_err(|e| format!("invalid metadata JSON: {e}"))?;
    let edges = check(&value)?;
    println!("architecture: {edges} declared normal/dev/build edges accepted");
    if real_workspace {
        let core_count = check_sources(std::path::Path::new("crates/core/src"))?;
        if core_count == 0 {
            return Err(
                "core source scan found no Rust files; run from the repository root".into(),
            );
        }
        let mut count = core_count;
        for package in value["packages"]
            .as_array()
            .ok_or("metadata lacks packages")?
        {
            let name = package["name"].as_str().ok_or("package lacks name")?;
            if name.starts_with("unisphere-adapter-") {
                let manifest = package["manifest_path"]
                    .as_str()
                    .ok_or("package lacks manifest")?;
                let source = std::path::Path::new(manifest)
                    .parent()
                    .ok_or("manifest lacks parent")?
                    .join("src");
                let checked = check_sources(&source)?;
                if checked == 0 {
                    return Err(format!("adapter source scan found no Rust files: {name}"));
                }
                count += checked;
            }
        }
        println!(
            "purity: {count} core/adapter production source files checked (lexical sensor; independent review still required)"
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("architecture: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn accepts_only_declared_allowed_edges_in_a_partial_workspace() {
        assert!(
            check(&fixture(include_str!(
                "../../fixtures/architecture/allowed.json"
            )))
            .is_ok()
        );
    }

    #[test]
    fn query_adapters_cannot_depend_outward_on_services_or_cli() {
        for (package, forbidden) in [
            ("unisphere-loader-query", "unisphere-sdk"),
            ("unisphere-output-query", "unisphere-cli"),
        ] {
            let mut graph = fixture(include_str!("../../fixtures/architecture/allowed.json"));
            let adapter = graph["packages"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|entry| entry["name"] == package)
                .unwrap();
            adapter["dependencies"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "name": forbidden, "kind": null, "path": "../forbidden"
                }));
            let error = check(&graph).unwrap_err();
            assert!(
                error.contains(&format!("{package} -> {forbidden}")),
                "{error}"
            );
        }
    }

    #[test]
    fn purity_sensor_rejects_negative_sources_but_permits_core_ports() {
        let cases: Value =
            serde_json::from_str(include_str!("../../fixtures/architecture/purity.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let result = check_source(
                case["source"].as_str().unwrap(),
                case["name"].as_str().unwrap(),
            );
            assert_eq!(
                result.is_ok(),
                case["allowed"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn forbidden_graph_fixtures_are_rejected_with_the_edge_kind() {
        for (text, expected) in [
            (
                include_str!("../../fixtures/architecture/core-to-cli.json"),
                "forbidden normal dependency: unisphere-core -> unisphere-cli",
            ),
            (
                include_str!("../../fixtures/architecture/build-to-sdk.json"),
                "forbidden build dependency: unisphere-core -> unisphere-sdk",
            ),
            (
                include_str!("../../fixtures/architecture/shipped-testkit.json"),
                "forbidden normal dependency: unisphere-sdk -> unisphere-testkit",
            ),
            (
                include_str!("../../fixtures/architecture/dev-reverse.json"),
                "forbidden dev dependency: unisphere-core -> unisphere-cli",
            ),
        ] {
            assert!(check(&fixture(text)).unwrap_err().contains(expected));
        }
    }

    #[test]
    fn malformed_or_incomplete_graphs_never_pass() {
        assert!(check(&serde_json::json!({})).is_err());
        assert!(check(&serde_json::json!({"packages":[],"workspace_members":[]})).is_err());
        let mut graph = fixture(include_str!("../../fixtures/architecture/allowed.json"));
        graph["packages"][0]["dependencies"][0]["kind"] = serde_json::json!("future");
        assert!(
            check(&graph)
                .unwrap_err()
                .contains("unknown dependency kind")
        );
        graph["workspace_members"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("missing"));
        assert!(check(&graph).is_err());
    }
}
