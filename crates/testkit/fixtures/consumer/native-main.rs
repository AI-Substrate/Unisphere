use std::{env, fs, io::{self, Write}, path::{Path, PathBuf}};
use serde_json::{Value, json};
use unisphere_sdk::*;
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_loader_snapshot::FileSnapshotLoader;
use unisphere_output_otlp::OtlpJsonlWriter;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn jsonl<A: SessionAdapter>(adapter: A, path: PathBuf, content: bool, out: &mut dyn Write) -> Result<()> {
    let service = Collector::new(FileSessionLoader, adapter, OtlpJsonlWriter);
    let source = SessionRef { path };
    let limits = ReadLimits { max_records: 1, ..ReadLimits::default() };
    let mut cursor = None;
    loop {
        let batch = service.collect_batch(&source, cursor.as_ref(), limits,
            MappingOptions { include_content: content }, out)?;
        cursor = Some(batch.next_cursor);
        if !batch.more { break; }
    }
    Ok(())
}

fn snapshot<A: SnapshotAdapter>(adapter: A, source: SnapshotRef, content: bool, out: &mut dyn Write) -> Result<()> {
    SnapshotCollector::new(FileSnapshotLoader, adapter, OtlpJsonlWriter).collect_snapshot(
        &SnapshotRequest { source, limits: SnapshotLimits::default(), options: MappingOptions { include_content: content } }, out)?;
    Ok(())
}

fn export(id: &str, path: PathBuf, content: bool, format: &str, out: &mut dyn Write) -> Result<()> {
    let source = SnapshotRef { path: path.clone(), format: match format {
        "json-journal" => SnapshotFormat::JsonJournal,
        "sqlite-key-value" => SnapshotFormat::SqliteKeyValue { table: "cursorDiskKV".into() },
        _ => SnapshotFormat::JsonDocument,
    }, session_id: None };
    match id {
        "claude-code" => jsonl(unisphere_adapter_claude::ClaudeCodeAdapter, path, content, out),
        "codex" => jsonl(unisphere_adapter_codex::CodexAdapter, path, content, out),
        "oh-my-pi" => jsonl(unisphere_adapter_omp::OmpAdapter, path, content, out),
        "pi" => jsonl(unisphere_adapter_pi::PiAdapter, path, content, out),
        "copilot-cli" => jsonl(unisphere_adapter_copilot_cli::CopilotCliAdapter, path, content, out),
        "cursor-transcript" => jsonl(unisphere_adapter_cursor::CursorAdapter, path, content, out),
        "copilot-cli-snapshot" => snapshot(unisphere_adapter_copilot_cli::CopilotCliAdapterSnapshot, source, content, out),
        "vscode-copilot" => snapshot(unisphere_adapter_vscode_copilot::VsCodeCopilotAdapter, source, content, out),
        "cursor-ide" => snapshot(unisphere_adapter_cursor::CursorIdeAdapter, source, content, out),
        _ => Err("unknown proof adapter".into()),
    }
}

fn seed_cursor(fixture: &Path, path: &Path) -> Result<()> {
    let rows: Value = serde_json::from_slice(&fs::read(fixture)?)?;
    let mut db = rusqlite::Connection::open(path)?;
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY, value BLOB)")?;
    let tx = db.transaction()?;
    for row in rows.as_array().ok_or("fixture is not an array")? {
        tx.execute("INSERT INTO cursorDiskKV VALUES (?1, ?2)", rusqlite::params![
            row["key"].as_str().ok_or("fixture key missing")?, serde_json::to_vec(&row["value"])?])?;
    }
    tx.commit()?;
    Ok(())
}

fn change_cursor(path: &Path, action: &str) -> Result<()> {
    let mut db = rusqlite::Connection::open(path)?;
    let tx = db.transaction()?;
    match action {
        "late" => {
            let raw: Vec<u8> = tx.query_row("SELECT value FROM cursorDiskKV WHERE key='composerData:alpha'", [], |r| r.get(0))?;
            let mut composer: Value = serde_json::from_slice(&raw)?;
            composer["fullConversationHeadersOnly"].as_array_mut().ok_or("missing headers")?.push(json!({"bubbleId":"late-proof","type":1}));
            tx.execute("UPDATE cursorDiskKV SET value=?1 WHERE key='composerData:alpha'", [serde_json::to_vec(&composer)?])?;
            tx.execute("INSERT INTO cursorDiskKV VALUES ('bubbleId:alpha:late-proof',?1)", [serde_json::to_vec(&json!({"_v":2,"bubbleId":"late-proof","type":1,"text":"SENSITIVE-LATE-PROOF"}))?])?;
        }
        "update" => {
            let raw: Vec<u8> = tx.query_row("SELECT value FROM cursorDiskKV WHERE key='bubbleId:alpha:a'", [], |r| r.get(0))?;
            let mut bubble: Value = serde_json::from_slice(&raw)?;
            bubble["text"] = json!("SENSITIVE-REVISED-PROOF");
            tx.execute("UPDATE cursorDiskKV SET value=?1 WHERE key='bubbleId:alpha:a'", [serde_json::to_vec(&bubble)?])?;
        }
        "delete" => { tx.execute("DELETE FROM cursorDiskKV", [])?; }
        _ => return Err("unknown fixture mutation".into()),
    }
    tx.commit()?;
    Ok(())
}

struct PartialThenFail { bytes: Vec<u8> }
impl Write for PartialThenFail {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.bytes.is_empty() { return Err(io::ErrorKind::BrokenPipe.into()); }
        let count = bytes.len().min(11);
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

fn failed_checkpoint(path: PathBuf) -> Result<()> {
    let service = SnapshotCollector::new(FileSnapshotLoader,
        unisphere_adapter_vscode_copilot::VsCodeCopilotAdapter, OtlpJsonlWriter);
    let request = SnapshotRequest { source: SnapshotRef { path, format: SnapshotFormat::JsonDocument, session_id: None },
        limits: SnapshotLimits::default(), options: MappingOptions::default() };
    let mut destination = PartialThenFail { bytes: Vec::new() };
    let failed = service.collect_snapshot(&request, &mut destination);
    if !matches!(failed, Err(ref error) if error.kind() == PipelineErrorKind::Write) || destination.bytes.len() != 11 {
        return Err("real partial output did not fail before checkpoint publication".into());
    }
    let mut accepted = Vec::new();
    let result = service.collect_snapshot(&request, &mut accepted)?;
    println!("{}", json!({"checkpoint_on_failure":false,"partial_bytes":11,
        "retry_revision":result.checkpoint.revision,"retry_records":result.records_written}));
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("seed-cursor") if args.len() == 3 => seed_cursor(Path::new(&args[1]), Path::new(&args[2])),
        Some("change-cursor") if args.len() == 3 => change_cursor(Path::new(&args[1]), &args[2]),
        Some("failure-probe") if args.len() == 2 => failed_checkpoint(args[1].clone().into()),
        Some("export") if args.len() == 5 => export(&args[1], args[2].clone().into(), args[3] == "content", &args[4], &mut io::stdout().lock()),
        _ => Err("usage: native-consumer export ID PATH metadata|content FORMAT; seed-cursor FIXTURE DB; change-cursor DB ACTION; failure-probe JSON".into()),
    }
}
