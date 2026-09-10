//! Installed query recipe over ordinary Git, with Git AI absent.
#[cfg(unix)]
pub fn run(
    cli: &std::path::Path,
    source: &std::path::Path,
    git: &std::path::Path,
    scratch: &std::path::Path,
) -> super::ProofResult<()> {
    use serde_json::Value;
    use std::{ffi::OsString, fs, os::unix::fs::symlink};
    use unisphere_testkit::sealed_command;
    fs::create_dir_all(scratch.join("bin")).map_err(|e| e.to_string())?;
    symlink(git, scratch.join("bin/git")).map_err(|e| e.to_string())?;
    let invoke = |args: &[&str]| -> super::ProofResult<std::process::Output> {
        let mut command = sealed_command(cli, scratch).map_err(|e| e.to_string())?;
        command
            .current_dir(source)
            .env("PATH", scratch.join("bin"))
            .args(args);
        super::capture(&mut command)
    };
    let output = invoke(&[
        "sessions",
        "list",
        "--repo",
        ".",
        "--source-adapter",
        "git-ai",
        "--columns",
        "transcript_available",
        "--format",
        "json",
    ])?;
    super::expect_status(&output, 0, "installed Git attribution recipe")?;
    let sessions: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    if sessions["data"]["emitted"] != 1
        || sessions["data"]["rows"][0]["fields"]["transcript_available"] != false
        || String::from_utf8_lossy(&output.stdout).contains("SENSITIVE")
        || !output.stderr.is_empty()
    {
        return Err("Git query fabricated transcript or leaked content".into());
    }
    let source_id = sessions["data"]["rows"][0]["source_refs"][0]["source_id"]
        .as_str()
        .ok_or("note source ID missing")?;
    let session_id = sessions["data"]["rows"][0]["id"]
        .as_str()
        .ok_or("attribution session ID missing")?;
    let events = invoke(&[
        "events",
        "list",
        "--source",
        source_id,
        "--source-adapter",
        "git-ai",
        "--limit",
        "0",
        "--format",
        "jsonl",
    ])?;
    super::expect_status(&events, 0, "installed note events")?;
    let rows = events
        .stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice::<Value>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if rows.is_empty()
        || rows
            .iter()
            .any(|row| row.get("timestamp").is_some() || row.get("next_action").is_some())
    {
        return Err("note events invented time or guidance rows".into());
    }
    let detailed = invoke(&[
        "events",
        "extract",
        "--source",
        source_id,
        "--source-adapter",
        "git-ai",
        "--include-content",
        "--columns",
        "parts",
        "--json",
    ])?;
    super::expect_status(&detailed, 0, "consented note attribution details")?;
    if !String::from_utf8_lossy(&detailed.stdout).contains("target_commit") {
        return Err("consented note facts lost their attribution target".into());
    }
    for dataset in ["turns", "messages", "tools"] {
        let output = invoke(&[
            dataset,
            "list",
            "--source",
            source_id,
            "--source-adapter",
            "git-ai",
            "--session",
            session_id,
            "--json",
        ])?;
        super::expect_status(&output, 0, "note unavailable conversation dataset")?;
        if serde_json::from_slice::<Value>(&output.stdout).map_err(|e| e.to_string())?["data"]["emitted"]
            != 0
        {
            return Err("note attribution invented conversation rows".into());
        }
    }
    let saved = scratch.join("saved-git-sessions.json");
    fs::write(&saved, &output.stdout).map_err(|e| e.to_string())?;
    let offline = super::run_product(
        cli,
        &scratch.join("offline"),
        &[
            OsString::from("sessions"),
            "list".into(),
            "--input".into(),
            saved.into_os_string(),
            "--columns".into(),
            "transcript_available".into(),
            "--json".into(),
        ],
    )?;
    super::expect_status(&offline, 0, "offline attribution view without Git")?;
    if serde_json::from_slice::<Value>(&offline.stdout).map_err(|e| e.to_string())?["data"]["rows"]
        [0]["id"]
        != session_id
    {
        return Err("offline note identity changed".into());
    }
    println!(
        "Git query proof: installed recipe7, pinned attribution source/session, consent-qualified events, no transcript/turn/message/tool/timestamp invention, and offline view with no Git capability"
    );
    Ok(())
}
