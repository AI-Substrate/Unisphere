#![forbid(unsafe_code)]

use serde_json::Value;
use std::{collections::BTreeSet, env, fs, path::PathBuf, process::{Command, ExitCode}};

// Check declarations, not only resolved/active edges: optional and target-specific
// dependencies must obey the same boundary even on a machine that never builds them.
fn allowed(package: &str, dependency: &str, kind: &str) -> bool {
    match (package, kind) {
        ("unisphere-core", "normal") => dependency == "serde",
        ("unisphere-sdk", "normal") => matches!(dependency, "unisphere-core" | "serde" | "serde_json"),
        ("unisphere-cli", "normal") => matches!(dependency, "unisphere-core" | "clap" | "serde_json"),
        ("unisphere-app", "normal") => matches!(dependency, "unisphere-sdk" | "unisphere-cli"),
        ("unisphere-testkit", "normal") => matches!(dependency, "unisphere-core" | "serde_json" | "tempfile"),
        ("unisphere-sdk" | "unisphere-cli" | "unisphere-app", "dev") => dependency == "unisphere-testkit",
        _ => false,
    }
}

fn check(graph: &Value) -> Result<usize, String> {
    let packages = graph["packages"].as_array().ok_or("metadata lacks packages array")?;
    let members = graph["workspace_members"].as_array().ok_or("metadata lacks workspace_members array")?;
    if members.is_empty() { return Err("workspace contains no members".into()); }
    let mut names = BTreeSet::new();
    let mut count = 0;
    for member in members {
        let id = member.as_str().ok_or("workspace member id is not a string")?;
        let package = packages.iter().find(|p| p["id"].as_str() == Some(id))
            .ok_or_else(|| format!("workspace member {id} has no package record"))?;
        let name = package["name"].as_str().ok_or("package lacks name")?;
        if !matches!(name, "unisphere-core" | "unisphere-sdk" | "unisphere-cli" | "unisphere-app" | "unisphere-testkit") {
            return Err(format!("unapproved workspace package {name}"));
        }
        if !names.insert(name) { return Err(format!("duplicate workspace package {name}")); }
        for dependency in package["dependencies"].as_array().ok_or("package lacks dependencies array")? {
            let target = dependency["name"].as_str().ok_or("dependency lacks package name")?;
            let kind = match dependency.get("kind") {
                Some(Value::Null) => "normal",
                Some(Value::String(kind)) if matches!(kind.as_str(), "normal" | "dev" | "build") => kind,
                _ => return Err(format!("{name} -> {target}: missing or unknown dependency kind")),
            };
            if !allowed(name, target, kind) {
                return Err(format!("forbidden {kind} dependency: {name} -> {target} (target={}, optional={}, rename={})", dependency["target"], dependency["optional"], dependency["rename"]));
            }
            // A renamed dependency still uses Cargo's actual package name above.
            // A registry/git substitute for an internal contract is never allowed.
            if target.starts_with("unisphere-") && !dependency["path"].is_string() {
                return Err(format!("{name} -> {target}: internal dependency must use a local path"));
            }
            count += 1;
        }
    }
    for required in ["unisphere-core", "unisphere-testkit"] {
        if !names.contains(required) { return Err(format!("required baseline package {required} absent")); }
    }
    Ok(count)
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let graph = match args.next().as_deref() {
        None => {
            let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
                .output().map_err(|e| format!("cargo metadata could not start: {e}"))?;
            if !output.status.success() {
                return Err(format!("cargo metadata failed: {}\nstdout:\n{}\nstderr:\n{}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)));
            }
            output.stdout
        }
        Some(flag) if flag == "--metadata" => {
            let path = PathBuf::from(args.next().ok_or("--metadata requires a path")?);
            if args.next().is_some() { return Err("usage: unisphere-arch-check [--metadata FILE]".into()); }
            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?
        }
        _ => return Err("usage: unisphere-arch-check [--metadata FILE]".into()),
    };
    let value = serde_json::from_slice(&graph).map_err(|e| format!("invalid metadata JSON: {e}"))?;
    let edges = check(&value)?;
    println!("architecture: {edges} declared normal/dev/build edges accepted");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("architecture: {error}"); ExitCode::FAILURE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(text: &str) -> Value { serde_json::from_str(text).unwrap() }

    #[test]
    fn accepts_only_declared_allowed_edges_in_a_partial_workspace() {
        assert_eq!(check(&fixture(include_str!("../../fixtures/architecture/allowed.json"))).unwrap(), 7);
    }

    #[test]
    fn forbidden_graph_fixtures_are_rejected_with_the_edge_kind() {
        for (text, expected) in [
            (include_str!("../../fixtures/architecture/core-to-cli.json"), "forbidden normal dependency: unisphere-core -> unisphere-cli"),
            (include_str!("../../fixtures/architecture/build-to-sdk.json"), "forbidden build dependency: unisphere-core -> unisphere-sdk"),
            (include_str!("../../fixtures/architecture/shipped-testkit.json"), "forbidden normal dependency: unisphere-sdk -> unisphere-testkit"),
            (include_str!("../../fixtures/architecture/dev-reverse.json"), "forbidden dev dependency: unisphere-core -> unisphere-cli"),
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
        assert!(check(&graph).unwrap_err().contains("unknown dependency kind"));
        graph["workspace_members"].as_array_mut().unwrap().push(serde_json::json!("missing"));
        assert!(check(&graph).is_err());
    }
}
