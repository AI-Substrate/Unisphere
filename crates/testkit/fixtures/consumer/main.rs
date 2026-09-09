#![forbid(unsafe_code)]

use serde_json::{Value, json};
use std::{path::Path, process::ExitCode};
use unisphere_sdk::{ConfigOverrides, ConfigReader, ConfigSource, Configuration, Failure,
    InspectionApi, InspectionReport, InspectionRequest, Inspector, ReadFailure};

fn project(result: Result<InspectionReport, Failure>) -> Value {
    match result {
        Ok(report) => json!({"ok":true,"command":"config.check","v":1,"data":report}),
        Err(error) => json!({"ok":false,"command":"config.check","v":1,"error":{
            "kind":error.kind(),"code":error.code(),"message":error.message(),
            "fix":error.fix(),"retryable":error.retryable(),"location":error.location()
        }}),
    }
}

struct Reader(Result<Vec<u8>, ReadFailure>);
impl ConfigReader for Reader {
    fn read(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, ReadFailure> {
        assert!(path.is_absolute());
        assert_eq!(max_bytes, 1_048_576);
        self.0.clone()
    }
}

fn self_check() -> Value {
    let defaults = InspectionRequest { source: ConfigSource::Defaults, overrides: ConfigOverrides::default() };
    let default_result = project(unisphere_sdk::inspect(&defaults));
    assert_eq!(default_result["data"]["configuration"]["source_roots"], json!([]));
    let configured = Inspector::with_defaults(Reader(Err(ReadFailure::Other)), Configuration { source_roots: vec!["default-root".into()] });
    assert_eq!(configured.inspect(&defaults).unwrap().configuration.source_roots, ["default-root"]);
    let mut request = InspectionRequest { source: ConfigSource::Inline(br#"{"source_roots":[" relative ","~/literal","same","same"]}"#.to_vec()), overrides: ConfigOverrides::default() };
    let explicit = project(unisphere_sdk::inspect(&request));
    assert_eq!(explicit["data"]["configuration"]["source_roots"], json!([" relative ","~/literal","same","same"]));
    request.overrides.source_roots = Some(vec![]);
    assert!(unisphere_sdk::inspect(&request).unwrap().configuration.source_roots.is_empty());
    request.source = ConfigSource::Inline(br#"{"unknown":"SENSITIVE-CONFIG-MARKER"}"#.to_vec());
    let invalid = project(unisphere_sdk::inspect(&request));
    assert_eq!(invalid["error"]["code"], "UNI-CONFIG-INVALID");
    assert!(!invalid.to_string().contains("SENSITIVE-CONFIG-MARKER"));
    let path = std::env::current_dir().unwrap().join("injected-only.json");
    request.source = ConfigSource::File(path);
    request.overrides.source_roots = None;
    let injected = Inspector::new(Reader(Ok(br#"{"source_roots":["injected"]}"#.to_vec())));
    assert_eq!(injected.inspect(&request).unwrap().configuration.source_roots, ["injected"]);
    let unreadable = Inspector::new(Reader(Err(ReadFailure::PermissionDenied))).inspect(&request).unwrap_err();
    assert_eq!(unreadable.read_failure(), Some(ReadFailure::PermissionDenied));
    // Do not return cwd-dependent locations: hostile-environment runs must have
    // byte-identical observable results despite having distinct temporary roots.
    json!({"defaults":default_result,"explicit":explicit,"invalid":invalid,"injected_roots":["injected"],"unreadable_code":unreadable.code()})
}

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(mode) = args.next() else { eprintln!("expected self-check, defaults, or file PATH [clear|override]"); return ExitCode::from(2); };
    if mode == "self-check" {
        println!("{}", self_check());
        return ExitCode::SUCCESS;
    }
    let source = if mode == "defaults" {
        ConfigSource::Defaults
    } else if mode == "file" {
        ConfigSource::File(args.next().expect("explicit config path").into())
    } else { eprintln!("unsupported consumer mode"); return ExitCode::from(2); };
    let source_roots = match args.next().as_deref() {
        None => None,
        Some(value) if value == "clear" => Some(vec![]),
        Some(value) if value == "override" => Some(vec!["override-root".into()]),
        _ => { eprintln!("unsupported override"); return ExitCode::from(2); }
    };
    assert!(args.next().is_none());
    let result = project(unisphere_sdk::inspect(&InspectionRequest { source, overrides: ConfigOverrides { source_roots } }));
    let success = result["ok"] == true;
    println!("{result}");
    if success { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
