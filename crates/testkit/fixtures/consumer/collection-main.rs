use std::{env, io, path::PathBuf, process::ExitCode};
use unisphere_sdk::{CollectionApi, Collector, MappingOptions, ReadLimits, SessionRef};
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_adapter_claude::ClaudeCodeAdapter;
use unisphere_output_otlp::OtlpJsonlWriter;

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let session = SessionRef { path: PathBuf::from(&args[0]) };
    let include_content = args.get(1).is_some_and(|arg| arg == "content");
    let max_records = args.get(2).and_then(|arg| arg.to_str()).and_then(|s| s.parse().ok()).unwrap_or(1);
    let max_record_bytes = args.get(3).and_then(|arg| arg.to_str()).and_then(|s| s.parse().ok()).unwrap_or(1_048_576);
    let limits = ReadLimits { max_records, max_record_bytes, max_batch_bytes: max_record_bytes.max(4_194_304) };
    let collector = Collector::new(FileSessionLoader::default(), ClaudeCodeAdapter::default(), OtlpJsonlWriter::default());
    let mut cursor = None;
    let stdout = io::stdout();
    let mut destination = stdout.lock();
    loop {
        match collector.collect_batch(&session, cursor.as_ref(), limits, MappingOptions { include_content }, &mut destination) {
            Ok(batch) => {
                cursor = Some(batch.next_cursor);
                if !batch.more {
                    eprintln!("{}", serde_json::json!({"incomplete_tail":batch.incomplete_tail,"offset":cursor.as_ref().map(|c| c.offset)}));
                    return ExitCode::SUCCESS;
                }
            }
            Err(error) => {
                eprintln!("{}", serde_json::json!({"code":error.code(),"offset":error.offset()}));
                return ExitCode::FAILURE;
            }
        }
    }
}
