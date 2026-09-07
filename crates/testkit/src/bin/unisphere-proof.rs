#![forbid(unsafe_code)]

use serde_json::{Value, json};
use std::{env, ffi::{OsStr, OsString}, fs, path::{Path, PathBuf}, process::{Command, ExitCode, Output}};
use unisphere_testkit::{fixtures, sealed_command};

type ProofResult<T> = Result<T, String>;

fn capture(command: &mut Command) -> ProofResult<Output> {
    command.output().map_err(|error| format!("could not start {command:?}: {error}"))
}

fn expect_status(output: &Output, code: i32, label: &str) -> ProofResult<()> {
    if output.status.code() == Some(code) { return Ok(()); }
    Err(format!("{label}: expected exit {code}, got {}\nstdout:\n{}\nstderr:\n{}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
}

fn machine(output: &Output, code: i32, command: &str) -> ProofResult<Value> {
    expect_status(output, code, command)?;
    if !output.stderr.is_empty() || !output.stdout.ends_with(b"\n") || output.stdout.iter().filter(|byte| **byte == b'\n').count() != 1 {
        return Err(format!("{command}: expected one JSON object plus LF and empty stderr; stdout={:?}, stderr={:?}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)));
    }
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|e| format!("{command}: invalid JSON output: {e}"))?;
    if !value.is_object() || value["v"] != 1 || value["command"] != command || value["ok"] != (code == 0) {
        return Err(format!("{command}: unexpected envelope {value}"));
    }
    let (present, absent) = if code == 0 { ("data", "error") } else { ("error", "data") };
    if !value[present].is_object() || value.get(absent).is_some() {
        return Err(format!("{command}: invalid success/error discrimination {value}"));
    }
    if code != 0 && (value["error"]["retryable"] != false || !value["error"]["fix"].as_str().is_some_and(|s| !s.is_empty())) {
        return Err(format!("{command}: error lacks actionable fixed diagnostics {value}"));
    }
    if String::from_utf8_lossy(&output.stdout).contains("SENSITIVE-CONFIG-MARKER") {
        return Err(format!("{command}: diagnostic leaked input marker"));
    }
    Ok(value)
}

fn run_product(binary: &Path, root: &Path, args: &[OsString]) -> ProofResult<Output> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut command = sealed_command(binary, root).map_err(|e| e.to_string())?;
    capture(command.args(args))
}

// The compiler lane deliberately retains tool access, unlike product execution.
// Caches, HOME/config, target and installation writes are temporary. Existing
// rustup toolchains are read through an explicit RUSTUP_HOME, never copied.
fn cargo_command(repo: &Path, scratch: &Path) -> ProofResult<Command> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.env_clear().current_dir(repo);
    for key in ["PATH", "RUSTUP_TOOLCHAIN", "RUSTC", "RUSTDOC", "CC", "CXX", "AR", "SDKROOT", "DEVELOPER_DIR", "SystemRoot"] {
        if let Some(value) = env::var_os(key) { command.env(key, value); }
    }
    let rustup_home = env::var_os("RUSTUP_HOME").map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".rustup")));
    if let Some(path) = rustup_home { command.env("RUSTUP_HOME", path); }
    let compiler = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let sysroot = capture(Command::new(compiler).current_dir(repo).args(["--print", "sysroot"]))?;
    expect_status(&sysroot, 0, "selected compiler sysroot")?;
    let sysroot = PathBuf::from(String::from_utf8(sysroot.stdout).map_err(|e| e.to_string())?.trim());
    for (key, executable) in [("RUSTC", "rustc"), ("RUSTDOC", "rustdoc")] {
        let binary = sysroot.join("bin").join(format!("{executable}{}", env::consts::EXE_SUFFIX));
        require_file(&binary)?;
        command.env(key, binary);
    }
    for leaf in ["home", "config", "cache", "data", "state", "tmp", "cargo", "target"] {
        fs::create_dir_all(scratch.join(leaf)).map_err(|e| e.to_string())?;
    }
    command.env("HOME", scratch.join("home"))
        .env("USERPROFILE", scratch.join("home"))
        .env("XDG_CONFIG_HOME", scratch.join("config"))
        .env("XDG_CACHE_HOME", scratch.join("cache"))
        .env("XDG_DATA_HOME", scratch.join("data"))
        .env("XDG_STATE_HOME", scratch.join("state"))
        .env("CARGO_HOME", scratch.join("cargo"))
        .env("CARGO_TARGET_DIR", scratch.join("target"))
        .env("TMPDIR", scratch.join("tmp"))
        .env("TEMP", scratch.join("tmp"))
        .env("TMP", scratch.join("tmp"));
    Ok(command)
}

fn build(repo: &Path, scratch: &Path, args: &[&OsStr]) -> ProofResult<()> {
    let mut command = cargo_command(repo, scratch)?;
    let output = capture(command.args(args))?;
    expect_status(&output, 0, "Cargo build/install")
}

fn require_file(path: &Path) -> ProofResult<()> {
    if path.is_file() { Ok(()) } else { Err(format!("required target absent: {}; compose the SDK/CLI/app before running this proof", path.display())) }
}

fn consumer(repo: &Path, scratch: &Path) -> ProofResult<PathBuf> {
    let sdk = repo.join("crates/sdk");
    require_file(&sdk.join("Cargo.toml"))?;
    let project = scratch.join("consumer");
    fs::create_dir_all(project.join("src")).map_err(|e| e.to_string())?;
    let sdk_text = sdk.to_str().ok_or("SDK path is not UTF-8")?;
    let manifest = include_str!("../../fixtures/consumer/Cargo.toml.template")
        .replace("@SDK_PATH@", &serde_json::to_string(sdk_text).map_err(|e| e.to_string())?);
    fs::write(project.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    fs::write(project.join("src/main.rs"), include_bytes!("../../fixtures/consumer/main.rs")).map_err(|e| e.to_string())?;
    build(repo, &scratch.join("build"), &[OsStr::new("build"), OsStr::new("--manifest-path"), project.join("Cargo.toml").as_os_str()])?;
    let binary = scratch.join("build/target/debug").join(format!("unisphere-external-consumer{}", env::consts::EXE_SUFFIX));
    require_file(&binary)?;
    Ok(binary)
}

fn hostile_consumer(binary: &Path, scratch: &Path) -> ProofResult<()> {
    let mut expected = None;
    for mode in ["baseline", "home", "xdg", "unisphere"] {
        let root = scratch.join(mode);
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let hostile = root.join("hostile");
        fs::create_dir_all(hostile.join("unisphere")).map_err(|e| e.to_string())?;
        for file in [hostile.join("config.json"), hostile.join(".unisphere.json"), hostile.join("unisphere/config.json")] {
            fs::write(file, br#"{"source_roots":["ambient-must-not-win"]}"#).map_err(|e| e.to_string())?;
        }
        let mut command = sealed_command(binary, &root).map_err(|e| e.to_string())?;
        match mode {
            "home" => { command.env("HOME", &hostile).env("USERPROFILE", &hostile); }
            "xdg" => {
                for key in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_CONFIG_DIRS", "XDG_DATA_DIRS", "APPDATA", "LOCALAPPDATA"] { command.env(key, &hostile); }
            }
            "unisphere" => { command.env("UNISPHERE_CONFIG", hostile.join("config.json")).env("UNISPHERE_CONFIG_PATH", hostile.join("config.json")).env("UNISPHERE_SOURCE_ROOTS", "ambient-must-not-win").env("UNISPHERE_OUTPUT", "human"); }
            _ => {}
        }
        let output = capture(command.arg("self-check"))?;
        expect_status(&output, 0, mode)?;
        if !output.stderr.is_empty() { return Err(format!("consumer {mode}: unexpected stderr {:?}", String::from_utf8_lossy(&output.stderr))); }
        let _: Value = serde_json::from_slice(&output.stdout).map_err(|e| format!("consumer {mode}: {e}"))?;
        if let Some(bytes) = &expected {
            if bytes != &output.stdout { return Err(format!("hostile {mode} environment influenced explicit SDK result")); }
        } else { expected = Some(output.stdout); }
    }
    Ok(())
}

fn strings(args: &[&str]) -> Vec<OsString> { args.iter().map(OsString::from).collect() }

fn config_args(path: Option<&Path>, mode: Option<&str>, human: bool) -> Vec<OsString> {
    let mut args = strings(&["config", "check"]);
    if let Some(path) = path { args.push("--config".into()); args.push(path.into()); }
    match mode {
        Some("clear") => args.push("--clear-source-roots".into()),
        Some("override") => args.extend(strings(&["--source-root", "override-root"])),
        _ => {}
    }
    if human { args.push("--human".into()); }
    args
}

fn parity_case(cli: &Path, sdk: &Path, scratch: &Path, name: &str, path: Option<&Path>, mode: Option<&str>, code: i32) -> ProofResult<()> {
    let mut sdk_args = if let Some(path) = path { vec![OsString::from("file"), path.into()] } else { strings(&["defaults"]) };
    if let Some(mode) = mode { sdk_args.push(mode.into()); }
    let sdk_output = run_product(sdk, &scratch.join(format!("{name}-sdk")), &sdk_args)?;
    let cli_output = run_product(cli, &scratch.join(format!("{name}-cli")), &config_args(path, mode, false))?;
    let sdk_value = machine(&sdk_output, code, "config.check")?;
    let cli_value = machine(&cli_output, code, "config.check")?;
    if sdk_value != cli_value { return Err(format!("{name}: SDK/CLI semantic mismatch\nSDK: {sdk_value}\nCLI: {cli_value}")); }
    if code == 0 {
        let expected = match mode {
            Some("clear") => json!([]),
            Some("override") => json!(fixtures::OVERRIDE_ROOTS),
            _ if name == "roots" => json!(fixtures::DOCUMENT_ROOTS),
            _ => json!([]),
        };
        if sdk_value["data"]["configuration"]["source_roots"] != expected { return Err(format!("{name}: incorrect effective roots {sdk_value}")); }
    }
    if code == 1 {
        let read_failure = matches!(name, "missing" | "unreadable" | "directory" | "oversized");
        let (kind, expected_code) = if read_failure { ("configuration_read", "UNI-CONFIG-READ") } else { ("invalid_configuration", "UNI-CONFIG-INVALID") };
        if sdk_value["error"]["kind"] != kind || sdk_value["error"]["code"] != expected_code {
            return Err(format!("{name}: wrong typed failure {sdk_value}"));
        }
        if let Some(path) = path {
            if sdk_value["error"]["location"]["path"].as_str() != path.to_str() {
                return Err(format!("{name}: missing explicit file location {sdk_value}"));
            }
        }
    }
    Ok(())
}

fn composition(repo: &Path, scratch: &Path) -> ProofResult<()> {
    require_file(&repo.join("crates/app/Cargo.toml"))?;
    let sdk = consumer(repo, scratch)?;
    build(repo, &scratch.join("build"), &[OsStr::new("build"), OsStr::new("--locked"), OsStr::new("-p"), OsStr::new("unisphere-app")])?;
    let cli = scratch.join("build/target/debug").join(format!("unisphere{}", env::consts::EXE_SUFFIX));
    require_file(&cli)?;
    parity_case(&cli, &sdk, scratch, "defaults", None, None, 0)?;
    for (name, bytes) in [("empty", fixtures::EMPTY), ("roots", fixtures::ROOTS)] {
        let path = scratch.join(format!("{name}.json"));
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
        parity_case(&cli, &sdk, scratch, name, Some(&path), None, 0)?;
        for mode in ["override", "clear"] { parity_case(&cli, &sdk, scratch, &format!("{name}-{mode}"), Some(&path), Some(mode), 0)?; }
    }
    for (name, bytes, _) in fixtures::INVALID_DOCUMENTS {
        let path = scratch.join(format!("{name}.json"));
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
        parity_case(&cli, &sdk, scratch, name, Some(&path), None, 1)?;
        parity_case(&cli, &sdk, scratch, &format!("{name}-overridden"), Some(&path), Some("clear"), 1)?;
    }
    parity_case(&cli, &sdk, scratch, "missing", Some(&scratch.join("absent.json")), None, 1)?;
    let oversized = scratch.join("oversized.json");
    fs::write(&oversized, fixtures::oversized_document()).map_err(|e| e.to_string())?;
    parity_case(&cli, &sdk, scratch, "oversized", Some(&oversized), None, 1)?;
    // Directories are deterministically unreadable as config documents, including
    // privileged CI. A separate POSIX mode-000 case proves PermissionDenied.
    let directory = scratch.join("not-a-file");
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    parity_case(&cli, &sdk, scratch, "directory", Some(&directory), None, 1)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let unreadable = scratch.join("unreadable.json");
        fs::write(&unreadable, fixtures::EMPTY).map_err(|e| e.to_string())?;
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0)).map_err(|e| e.to_string())?;
        let result = parity_case(&cli, &sdk, scratch, "unreadable", Some(&unreadable), None, 1);
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        result.map_err(|e| format!("permission proof requires an unprivileged POSIX user: {e}"))?;
    }
    hostile_consumer(&sdk, &scratch.join("hostile-sdk"))
}

fn installed_cli(repo: &Path, scratch: &Path) -> ProofResult<()> {
    let app = repo.join("crates/app");
    require_file(&app.join("Cargo.toml"))?;
    let install = scratch.join("install");
    build(repo, &scratch.join("build"), &[OsStr::new("install"), OsStr::new("--locked"), OsStr::new("--path"), app.as_os_str(), OsStr::new("--root"), install.as_os_str()])?;
    let cli = install.join("bin").join(format!("unisphere{}", env::consts::EXE_SUFFIX));
    require_file(&cli)?;
    for (flag, command, field) in [("--help", "help", "text"), ("--version", "version", "version")] {
        let output = run_product(&cli, &scratch.join(command), &strings(&[flag]))?;
        let value = machine(&output, 0, command)?;
        if !value["data"][field].as_str().is_some_and(|s| !s.is_empty()) { return Err(format!("{command}: missing {field}")); }
        let human = run_product(&cli, &scratch.join(format!("human-{command}")), &strings(&[flag, "--human"]))?;
        expect_status(&human, 0, command)?;
        if human.stdout.is_empty() || !human.stderr.is_empty() { return Err(format!("human {command}: wrong output routing")); }
    }
    let valid = scratch.join("roots.json");
    let invalid = scratch.join("invalid.json");
    fs::write(&valid, fixtures::ROOTS).map_err(|e| e.to_string())?;
    fs::write(&invalid, fixtures::UNKNOWN_KEY).map_err(|e| e.to_string())?;
    for (name, path, code) in [("success", &valid, 0), ("invalid", &invalid, 1), ("missing", &scratch.join("missing.json"), 1)] {
        let output = run_product(&cli, &scratch.join(format!("machine-{name}")), &config_args(Some(path), None, false))?;
        let value = machine(&output, code, "config.check")?;
        if code == 0 && value["data"]["configuration"]["source_roots"] != json!(fixtures::DOCUMENT_ROOTS) { return Err("installed CLI changed uninterpreted roots".into()); }
        let human = run_product(&cli, &scratch.join(format!("human-{name}")), &config_args(Some(path), None, true))?;
        expect_status(&human, code, "human config.check")?;
        let (message, empty) = if code == 0 { (&human.stdout, &human.stderr) } else { (&human.stderr, &human.stdout) };
        if message.is_empty() || !empty.is_empty() || String::from_utf8_lossy(message).contains("SENSITIVE-CONFIG-MARKER") {
            return Err(format!("human {name}: wrong stdout/stderr routing or leaked input; stdout={:?}, stderr={:?}", String::from_utf8_lossy(&human.stdout), String::from_utf8_lossy(&human.stderr)));
        }
        if code == 0 && fixtures::DOCUMENT_ROOTS.iter().any(|root| !String::from_utf8_lossy(message).contains(root)) {
            return Err("human success omitted effective configuration roots".into());
        }
        if code != 0 {
            for field in ["code", "message", "fix"] {
                let expected = value["error"][field].as_str().ok_or("missing machine diagnostic")?;
                if !String::from_utf8_lossy(message).contains(expected) { return Err(format!("human {name}: omitted {field}")); }
            }
        }
    }
    let invalid_args = run_product(&cli, &scratch.join("invalid-args"), &strings(&["config", "check", "--clear-source-roots", "--source-root", "root"]))?;
    machine(&invalid_args, 2, "config.check")?;
    Ok(())
}

fn run() -> ProofResult<()> {
    let mut args = env::args_os().skip(1);
    let mode = args.next().ok_or("usage: unisphere-proof composition|sdk-consumer|installed-cli [--repo ROOT]")?;
    let repo = match args.next().as_deref() {
        None => env::current_dir().map_err(|e| e.to_string())?,
        Some(flag) if flag == "--repo" => PathBuf::from(args.next().ok_or("--repo requires a root")?),
        _ => return Err("expected --repo ROOT".into()),
    };
    if args.next().is_some() { return Err("unexpected trailing arguments".into()); }
    let repo = fs::canonicalize(repo).map_err(|e| e.to_string())?;
    require_file(&repo.join("Cargo.toml"))?;
    let scratch = tempfile::Builder::new().prefix("unisphere-proof-").tempdir().map_err(|e| e.to_string())?;
    let result = match mode.to_str() {
        Some("composition") => composition(&repo, scratch.path()),
        Some("sdk-consumer") => consumer(&repo, scratch.path()).and_then(|binary| hostile_consumer(&binary, &scratch.path().join("runtime"))),
        Some("installed-cli") => installed_cli(&repo, scratch.path()),
        _ => Err("unknown proof command".into()),
    };
    result?;
    println!("foundation proof {}: passed (temporary isolated build and product runtime; no telemetry-reader claim)", mode.to_string_lossy());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("foundation proof failed: {error}"); ExitCode::FAILURE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn output(script: &str) -> Output {
        capture(Command::new("/bin/sh").args(["-c", script])).unwrap()
    }

    #[test]
    #[cfg(unix)]
    fn child_failure_preserves_status_and_both_streams() {
        let child = output("printf child-out; printf child-err >&2; exit 17");
        let error = expect_status(&child, 0, "controlled child").unwrap_err();
        assert!(error.contains("17"));
        assert!(error.contains("child-out"));
        assert!(error.contains("child-err"));
    }

    #[test]
    #[cfg(unix)]
    fn rejects_nonmachine_output_and_wrong_discrimination() {
        for script in [
            "printf 'not JSON\\n'",
            "printf '{\"ok\":true,\"v\":1,\"command\":\"config.check\",\"data\":{}}\\n'; printf contamination >&2",
            "printf '{\"ok\":true,\"v\":1,\"command\":\"config.check\",\"data\":{},\"error\":{}}\\n'",
            "printf '{\"ok\":true,\"v\":1,\"command\":\"config.check\",\"data\":{}}\\n\\n'",
        ] { assert!(machine(&output(script), 0, "config.check").is_err()); }
        let good = output("printf '{\"ok\":true,\"v\":1,\"command\":\"config.check\",\"data\":{}}\\n'");
        assert!(machine(&good, 0, "config.check").is_ok());
    }

    #[test]
    fn absent_targets_fail_before_any_build() {
        let root = tempfile::tempdir().unwrap();
        let scratch = root.path().join("scratch");
        assert!(consumer(root.path(), &scratch).unwrap_err().contains("required target absent"));
        assert!(composition(root.path(), &scratch).unwrap_err().contains("required target absent"));
        assert!(installed_cli(root.path(), &scratch).unwrap_err().contains("required target absent"));
        assert!(!scratch.exists());
    }
}
