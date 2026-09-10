use super::{ProofResult, expect_status, run_product, strings};
use serde_json::{Value, json};
use std::{ffi::OsString, fs, path::Path};

fn json_pair(cli: &Path, installed: &Path, scratch: &Path, args: &[OsString]) -> ProofResult<Value> {
    let mut previous = None;
    for (label, binary) in [("built", cli), ("installed", installed)] {
        let output = run_product(binary, &scratch.join(label), args)?;
        expect_status(&output, 0, "query runtime")?;
        if !output.stderr.is_empty() { return Err("successful JSON query emitted unexpected diagnostics".into()); }
        let value: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
        if value["ok"] != true { return Err("query success lacks ok envelope".into()); }
        if previous.as_ref().is_some_and(|prior| prior != &value) { return Err("built and installed query results differ".into()); }
        previous = Some(value);
    }
    previous.ok_or_else(|| "query binaries absent".into())
}

pub fn run(cli: &Path, installed: &Path, scratch: &Path) -> ProofResult<()> {
    fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let source = scratch.join("session.jsonl");
    let records = [
        json!({"type":"user","sessionId":"query-proof","uuid":"u1","cwd":scratch,"timestamp":"2026-09-01T12:00:00Z","message":{"role":"user","content":"fixture PRIVATE-QUERY-PROMPT"}}),
        json!({"type":"assistant","sessionId":"query-proof","uuid":"a1","parentUuid":"u1","cwd":scratch,"timestamp":"2026-09-01T12:00:01Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call1","name":"Bash","input":{"command":"PRIVATE-QUERY-COMMAND"}}]}}),
        json!({"type":"user","sessionId":"query-proof","uuid":"r1","parentUuid":"a1","cwd":scratch,"timestamp":"2026-09-01T12:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call1","content":"PRIVATE-QUERY-RESULT","is_error":true}]}}),
        json!({"type":"user","sessionId":"query-proof","uuid":"u2","parentUuid":"r1","cwd":scratch,"timestamp":"2026-09-01T12:00:03Z","message":{"role":"user","content":"another fixture request"}}),
    ];
    let mut native_bytes = Vec::new();
    for record in records { serde_json::to_writer(&mut native_bytes,&record).map_err(|e|e.to_string())?; native_bytes.push(b'\n'); }
    fs::write(&source,&native_bytes).map_err(|e|e.to_string())?;
    let query_args = |parts:&[&str]| {
        let mut args=strings(parts);
        args.extend([OsString::from("--source"),source.clone().into_os_string(),OsString::from("--source-adapter"),OsString::from("claude-code")]);
        args
    };
    let sessions=json_pair(cli,installed,scratch,&query_args(&["sessions","list","--json"]))?;
    if sessions["data"]["matched"]!=1 || sessions.to_string().contains("PRIVATE-QUERY") { return Err("native session query identity/privacy failed".into()); }
    let session_id=sessions["data"]["rows"][0]["id"].as_str().ok_or("session ID absent")?;
    let lineage=json_pair(cli,installed,scratch,&query_args(&["sessions","tree",session_id,"--columns","parent_ids,branch_ids","--json"]))?;
    if !lineage["data"]["rows"][0]["fields"]["branch_ids"].is_array() { return Err("tree branch projection absent".into()); }
    let filtered=json_pair(cli,installed,scratch,&query_args(&["messages","list","--role","user","--since","2026-09-01","--until","2026-09-02","--contains","fixture","--json"]))?;
    if filtered["data"]["matched"]!=2 || filtered.to_string().contains("PRIVATE-QUERY") { return Err("content search incorrectly selected or exposed payload".into()); }
    let stats=json_pair(cli,installed,scratch,&query_args(&["tools","stats","--group-by","tool_family","--metric","count","--metric","failures","--metric","measured_count","--metric","p95_ms","--json"]))?;
    let fields=&stats["data"]["rows"][0]["fields"];
    if fields["count"]!=1 || fields["failures"]!=1 || fields["measured_count"]!=1 || fields["p95_ms"]!=1000.0 { return Err("native tool statistics differ from observed evidence".into()); }
    let threshold=json_pair(cli,installed,scratch,&query_args(&["tools","list","--min-duration","500.5","--json"]))?;
    if threshold["data"]["matched"]!=1 { return Err("finite duration predicate failed".into()); }
    for (label,binary) in [("built",cli),("installed",installed)] {
        let output=run_product(binary,&scratch.join(label),&query_args(&["turns","extract","--has-errors","--context-before","1","--context-after","1","--include-content","--format","jsonl"]))?;
        expect_status(&output,0,"context extraction")?;
        let rows=output.stdout.split(|byte|*byte==b'\n').filter(|line|!line.is_empty()).map(serde_json::from_slice::<Value>).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        if rows.len()!=2 || rows.iter().filter(|row|row["is_context"]==true).count()!=1 || rows.iter().filter(|row|row["is_context"]==false).count()!=1 || rows.iter().any(|row|row.get("next_action").is_some()) { return Err("context row/diagnostic separation failed".into()); }
        let summary:Value=serde_json::from_slice(&output.stderr).map_err(|e|e.to_string())?;
        if summary["data"]["matched"]!=1 || summary["data"]["emitted"]!=2 || !summary["data"]["coverage"].is_object() { return Err("context summary lost coverage or match counts".into()); }
    }
    let saved=scratch.join("saved.json");
    fs::write(&saved,serde_json::to_vec(&sessions).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let mut offline=strings(&["sessions","list","--input"]);offline.push(saved.into_os_string());offline.push("--json".into());
    let offline=json_pair(cli,installed,scratch,&offline)?;
    if offline["data"]["rows"][0]["id"]!=session_id || offline["data"]["universe"]["basis"]!="saved_selection" { return Err("offline query changed identity or input basis".into()); }
    let topics=json_pair(cli,installed,scratch,&strings(&["docs","list","--json"]))?;
    for topic in topics["data"]["topics"].as_array().ok_or("docs topic list missing")? {
        let id=topic["id"].as_str().ok_or("topic ID missing")?;
        let document=json_pair(cli,installed,scratch,&strings(&["docs","get",id,"--json"]))?;
        if document["data"]["id"]!=id || document["data"]["text"].as_str().is_none_or(str::is_empty) { return Err("offline topic failed to load".into()); }
    }
    let schema=json_pair(cli,installed,scratch,&strings(&["schema","show","tools","--json"]))?;
    if schema["data"]["dataset"]!="tools" { return Err("schema runtime does not expose queried dataset".into()); }
    if fs::read(&source).map_err(|e|e.to_string())?!=native_bytes { return Err("query proof mutated native source".into()); }
    println!("query proof: built/installed native source, lineage, time/text privacy, context, statistics, duration predicate, saved input and bundled docs/schema parity; Git Notes query not included");
    Ok(())
}
