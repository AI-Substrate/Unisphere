use std::{collections::BTreeMap, io::{self, Write}};

use serde_json::{Value, json};
use unisphere_core::query::{
    ActionReason, Completeness, ContentAccess, Coverage, CsvSafety, Dataset, Digest, EntityId,
    EntityKind, FieldId, FieldValue, OperationKind, OutputFormat, ProjectedRow, QueryAction,
    QueryDescription, QueryFailureCode, QueryOutputOptions, QueryResponse, QueryWriter,
    RecoveryAction, RenderedAction, ResultUniverse, UniverseBasis,
};
use unisphere_output_query::ProjectedQueryWriter;

fn response(fields: BTreeMap<FieldId, FieldValue>) -> QueryResponse {
    let row = ProjectedRow::new(
        Dataset::Messages,
        EntityId::derive(EntityKind::Message, [&b"message-1"[..]]),
        Vec::new(),
        fields,
        &ContentAccess {
            inspect_fields: Default::default(),
            emit_content: true,
        },
    )
    .unwrap();
    QueryResponse {
        schema_version: 1,
        dataset: Dataset::Messages,
        query: QueryDescription {
            dataset: Dataset::Messages,
            operation: OperationKind::Extract,
            scope_digest: Digest::of_bytes(b"scope"),
        },
        rows: vec![row],
        coverage: Coverage::default(),
        universe: ResultUniverse {
            source_view_digest: None,
            selection_digest: Digest::of_bytes(b"selection"),
            columns_digest: Digest::of_bytes(b"columns"),
            columns: vec![FieldId::Id, FieldId::SourceRefs, FieldId::Text, FieldId::Parts],
            applied_limit: None,
            rows_complete_for_selection: Completeness::Complete,
            partitions_complete: Completeness::Complete,
            basis: UniverseBasis::LiveView,
            bounded_by_input: false,
        },
        matched: 1,
        emitted: 1,
        next_cursor: None,
        next_action: QueryAction::ReadRecipe {
            topic: "SENSITIVE-SEMANTIC-SENTINEL".into(),
            reason: ActionReason::NextDetail,
        },
    }
}

fn options(format: OutputFormat) -> QueryOutputOptions {
    QueryOutputOptions {
        format,
        csv_safety: CsvSafety::Spreadsheet,
        max_output_bytes: 1024 * 1024,
        next_action: RenderedAction {
            summary: "Inspect the selected message".into(),
            argv: vec!["unisphere".into(), "messages".into(), "show".into()],
            required_inputs: vec!["ENTITY".into()],
        },
    }
}

fn write(response: &QueryResponse, options: &QueryOutputOptions) -> Vec<u8> {
    let mut bytes = Vec::new();
    ProjectedQueryWriter
        .write(response, options, &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn json_is_a_query_envelope_with_rendered_guidance_not_otlp() {
    let response = response(BTreeMap::from([(FieldId::Role, FieldValue::String("user".into()))]));
    let output = write(&response, &options(OutputFormat::Json));
    let value: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "messages.extract");
    assert_eq!(value["v"], 1);
    assert_eq!(value["data"]["schema_version"], 1);
    assert_eq!(value["data"]["dataset"], "messages");
    assert_eq!(value["data"]["rows"][0]["fields"]["role"], "user");
    assert_eq!(value["next_action"]["summary"], "Inspect the selected message");
    assert!(value.get("resourceLogs").is_none());
    assert!(value["data"].get("next_action").is_none());
    assert!(!String::from_utf8(output).unwrap().contains("SENSITIVE-SEMANTIC-SENTINEL"));
}

#[test]
fn jsonl_contains_only_flat_directly_addressable_rows() {
    let response = response(BTreeMap::from([
        (FieldId::Role, FieldValue::String("user".into())),
        (FieldId::Text, FieldValue::Null),
    ]));
    let output = write(&response, &options(OutputFormat::Jsonl));
    let lines: Vec<_> = std::str::from_utf8(&output).unwrap().lines().collect();
    assert_eq!(lines.len(), 1);
    let value: Value = serde_json::from_str(lines[0]).unwrap();

    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["dataset"], "messages");
    assert_eq!(value["role"], "user");
    assert_eq!(value["text"], Value::Null);
    assert!(value.get("fields").is_none());
    assert!(value.get("next_action").is_none());
}

#[test]
fn csv_is_rfc4180_lossy_and_spreadsheet_safe() {
    let response = response(BTreeMap::from([
        (FieldId::Text, FieldValue::String("  =SUM(1,2)".into())),
        (FieldId::Parts, FieldValue::Structured(json!({"value":"x"}))),
        (FieldId::TurnId, FieldValue::Null),
    ]));
    let output = String::from_utf8(write(&response, &options(OutputFormat::Csv))).unwrap();

    assert!(output.starts_with("schema_version,dataset,id,source_refs,text,parts,turn_id\r\n"));
    assert!(output.contains("\"'  =SUM(1,2)\""));
    assert!(output.contains("\"{\"\"value\"\":\"\"x\"\"}\""));
    assert!(output.ends_with(",\r\n"));
    assert!(!output.contains("next_action"));

    let mut raw = options(OutputFormat::Csv);
    raw.csv_safety = CsvSafety::Raw;
    let raw = String::from_utf8(write(&response, &raw)).unwrap();
    assert!(raw.contains("\"  =SUM(1,2)\""));
    assert!(!raw.contains("\"'  =SUM"));
}

#[test]
fn raw_csv_round_trips_multiline_tabs_controls_and_quotes() {
    let raw_value = "\tfirst\r\nsecond,\u{1b}\"quoted\"";
    let response = response(BTreeMap::from([(
        FieldId::Text,
        FieldValue::String(raw_value.into()),
    )]));
    let mut raw_options = options(OutputFormat::Csv);
    raw_options.csv_safety = CsvSafety::Raw;

    let records = parse_csv(&write(&response, &raw_options));
    let text_index = records[0]
        .iter()
        .position(|field| field == b"text")
        .unwrap();
    assert_eq!(records[1][text_index], raw_value.as_bytes());

    let spreadsheet = parse_csv(&write(&response, &options(OutputFormat::Csv)));
    assert_eq!(
        spreadsheet[1][text_index],
        [b"'".as_slice(), raw_value.as_bytes()].concat()
    );
}

#[test]
fn text_and_markdown_escape_controls_and_hostile_markup_without_guidance() {
    let sentinel = "line\n\u{1b}[31m<script>*bold*";
    let response = response(BTreeMap::from([(FieldId::Text, FieldValue::String(sentinel.into()))]));

    let text = String::from_utf8(write(&response, &options(OutputFormat::Text))).unwrap();
    assert!(text.contains("line\\n\\u001b[31m<script>*bold*"));
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains("Inspect the selected message"));

    let markdown = String::from_utf8(write(&response, &options(OutputFormat::Markdown))).unwrap();
    assert!(markdown.contains("line\\\\n\\\\u001b\\[31m&lt;script&gt;\\*bold\\*"));
    assert!(!markdown.contains("<script>"));
    assert!(!markdown.contains('\u{1b}'));
    assert!(!markdown.contains("next_action"));
}

#[test]
fn bound_refusal_happens_before_the_destination_is_touched() {
    let response = response(BTreeMap::from([(
        FieldId::Text,
        FieldValue::String("x".repeat(1024 * 1024)),
    )]));
    let mut options = options(OutputFormat::Json);
    options.max_output_bytes = 8;
    let mut destination = CountingWriter::default();

    let error = ProjectedQueryWriter
        .write(&response, &options, &mut destination)
        .unwrap_err();
    assert_eq!(error.kind(), QueryFailureCode::ResourceLimit);
    assert_eq!(destination.writes, 0);
    assert_eq!(destination.flushes, 0);
}

#[test]
fn late_destination_failure_reports_truthful_partial_output_recovery() {
    let response = response(BTreeMap::from([(FieldId::Role, FieldValue::String("user".into()))]));
    let mut destination = FailsAfter { remaining: 12, bytes: Vec::new() };

    let error = ProjectedQueryWriter
        .write(&response, &options(OutputFormat::Json), &mut destination)
        .unwrap_err();
    assert_eq!(error.kind(), QueryFailureCode::OutputFailure);
    assert_eq!(error.code(), "UNI-QUERY-OUTPUT");
    assert!(error.message().contains("partial bytes"));
    assert_eq!(
        error.recovery(),
        &RecoveryAction::ChooseNewOutput { discard_partial: true }
    );
    assert!(error.retryable());
    assert!(!destination.bytes.is_empty());
}

#[test]
fn flush_failure_is_not_reported_as_success() {
    let response = response(BTreeMap::new());
    let mut destination = FlushFailure(Vec::new());
    let error = ProjectedQueryWriter
        .write(&response, &options(OutputFormat::Jsonl), &mut destination)
        .unwrap_err();
    assert_eq!(error.kind(), QueryFailureCode::OutputFailure);
}

#[derive(Default)]
struct CountingWriter {
    writes: usize,
    flushes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

struct FailsAfter {
    remaining: usize,
    bytes: Vec<u8>,
}

impl Write for FailsAfter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let accepted = self.remaining.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..accepted]);
        self.remaining -= accepted;
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct FlushFailure(Vec<u8>);

impl Write for FlushFailure {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

fn parse_csv(input: &[u8]) -> Vec<Vec<Vec<u8>>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = Vec::new();
    let mut quoted = false;
    let mut index = 0;
    while index < input.len() {
        match input[index] {
            b'"' if quoted && input.get(index + 1) == Some(&b'"') => {
                field.push(b'"');
                index += 2;
            }
            b'"' => {
                quoted = !quoted;
                index += 1;
            }
            b',' if !quoted => {
                record.push(std::mem::take(&mut field));
                index += 1;
            }
            b'\r' if !quoted && input.get(index + 1) == Some(&b'\n') => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                index += 2;
            }
            byte => {
                field.push(byte);
                index += 1;
            }
        }
    }
    assert!(!quoted);
    assert!(field.is_empty() && record.is_empty());
    records
}
