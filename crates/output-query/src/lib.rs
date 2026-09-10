//! Bounded serialization of validated query projections.
//!
//! This crate receives projected rows and caller-rendered action metadata. It
//! does not read sources, reconstruct query state, authorize content, open
//! files, or produce OTLP. Raw row streams never contain guidance records.
#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
    io::{self, Write},
};

use serde::{Serialize, ser::SerializeMap};
use unisphere_core::query::{
    CsvSafety, FieldId, FieldValue, LimitKind, OutputFormat, ProjectedRow, QueryFailure,
    QueryFailureCode, QueryLimits, QueryOutputOptions, QueryResponse, QueryWriter, RecoveryAction,
};

/// Stateless writer for versioned query JSON, row-only JSONL, CSV, and human
/// extraction formats.
///
/// Serialization is staged inside `max_output_bytes` before the destination is
/// touched. A destination failure can still leave partial bytes; the returned
/// [`QueryFailure`] therefore always identifies incomplete output and requires
/// the caller to discard any partial destination.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProjectedQueryWriter;

impl QueryWriter for ProjectedQueryWriter {
    fn write(
        &self,
        response: &QueryResponse,
        options: &QueryOutputOptions,
        destination: &mut dyn Write,
    ) -> Result<(), QueryFailure> {
        validate(response, options)?;

        let mut staged = BoundedBuffer::new(options.max_output_bytes);
        let encoded = match options.format {
            OutputFormat::Json => encode_json(response, options, &mut staged),
            OutputFormat::Jsonl => encode_jsonl(response, &mut staged),
            OutputFormat::Csv => encode_csv(response, options.csv_safety, &mut staged),
            OutputFormat::Table => encode_table(response, &mut staged),
            OutputFormat::Text => encode_text(response, &mut staged),
            OutputFormat::Markdown => encode_markdown(response, &mut staged),
        };
        encoded.map_err(serialization_failure)?;

        destination
            .write_all(staged.as_slice())
            .and_then(|()| destination.flush())
            .map_err(|_| output_failure())
    }
}

fn validate(response: &QueryResponse, options: &QueryOutputOptions) -> Result<(), QueryFailure> {
    options.validate_for(response, &QueryLimits::HARD)?;
    response.coverage.validate()?;
    response.universe.validate()?;
    if response.schema_version != 1
        || response.query.dataset != response.dataset
        || response.emitted != response.rows.len() as u64
        || response.rows.iter().any(|row| {
            row.schema_version() != 1 || row.dataset() != response.dataset
        })
    {
        return Err(QueryFailure::invalid_data());
    }
    Ok(())
}

fn serialization_failure(error: io::Error) -> QueryFailure {
    if error.kind() == io::ErrorKind::FileTooLarge {
        QueryFailure::limit(LimitKind::OutputBytes)
    } else {
        QueryFailure::invalid_data()
    }
}

fn output_failure() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::OutputFailure,
        RecoveryAction::ChooseNewOutput {
            discard_partial: true,
        },
    )
    .retryable_after_recovery(true)
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl BoundedBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(8 * 1024)),
            limit,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Serialize)]
struct JsonEnvelope<'a> {
    ok: bool,
    command: String,
    v: u8,
    data: JsonData<'a>,
    next_action: &'a unisphere_core::query::RenderedAction,
}

#[derive(Serialize)]
struct JsonData<'a> {
    schema_version: u16,
    dataset: unisphere_core::query::Dataset,
    query: &'a unisphere_core::query::QueryDescription,
    rows: &'a [ProjectedRow],
    coverage: &'a unisphere_core::query::Coverage,
    universe: &'a unisphere_core::query::ResultUniverse,
    matched: u64,
    emitted: u64,
    next_cursor: &'a Option<String>,
}

fn encode_json(
    response: &QueryResponse,
    options: &QueryOutputOptions,
    output: &mut dyn Write,
) -> io::Result<()> {
    let envelope = JsonEnvelope {
        ok: true,
        command: format!("{}.{}", response.dataset, response.query.operation),
        v: 1,
        data: JsonData {
            schema_version: response.schema_version,
            dataset: response.dataset,
            query: &response.query,
            rows: &response.rows,
            coverage: &response.coverage,
            universe: &response.universe,
            matched: response.matched,
            emitted: response.emitted,
            next_cursor: &response.next_cursor,
        },
        next_action: &options.next_action,
    };
    serde_json::to_writer(&mut *output, &envelope).map_err(json_error)?;
    output.write_all(b"\n")
}

fn encode_jsonl(response: &QueryResponse, output: &mut dyn Write) -> io::Result<()> {
    for row in &response.rows {
        serde_json::to_writer(&mut *output, &FlatRow(row)).map_err(json_error)?;
        output.write_all(b"\n")?;
    }
    Ok(())
}

struct FlatRow<'a>(&'a ProjectedRow);

impl Serialize for FlatRow<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let row = self.0;
        let mut map = serializer.serialize_map(Some(4 + row.fields().len()))?;
        map.serialize_entry("schema_version", &row.schema_version())?;
        map.serialize_entry("dataset", &row.dataset())?;
        map.serialize_entry("id", &row.id())?;
        map.serialize_entry("source_refs", row.source_refs())?;
        for (field, value) in row.fields() {
            map.serialize_entry(field.as_str(), &WireValue(value))?;
        }
        map.end()
    }
}

struct WireValue<'a>(&'a FieldValue);

impl Serialize for WireValue<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            FieldValue::Null => serializer.serialize_none(),
            FieldValue::Bool(value) => serializer.serialize_bool(*value),
            FieldValue::Unsigned(value) => serializer.serialize_u64(*value),
            FieldValue::Integer(value) => serializer.serialize_i64(*value),
            FieldValue::Float(value) => serializer.serialize_f64(*value),
            FieldValue::String(value) => serializer.serialize_str(value),
            FieldValue::Timestamp(value) => value.serialize(serializer),
            FieldValue::Id(value) => value.serialize(serializer),
            FieldValue::IdList(value) => value.serialize(serializer),
            FieldValue::Strings(value) => value.serialize(serializer),
            FieldValue::Structured(value) => value.serialize(serializer),
        }
    }
}

fn encode_csv(
    response: &QueryResponse,
    safety: CsvSafety,
    output: &mut dyn Write,
) -> io::Result<()> {
    let columns = projected_columns(response);
    write_csv_cell(output, "schema_version", CsvSafety::Raw, false)?;
    output.write_all(b",")?;
    write_csv_cell(output, "dataset", CsvSafety::Raw, false)?;
    output.write_all(b",")?;
    write_csv_cell(output, "id", CsvSafety::Raw, false)?;
    output.write_all(b",")?;
    write_csv_cell(output, "source_refs", CsvSafety::Raw, false)?;
    for column in &columns {
        output.write_all(b",")?;
        write_csv_cell(output, column.as_str(), CsvSafety::Raw, false)?;
    }
    output.write_all(b"\r\n")?;

    for row in &response.rows {
        write!(output, "{}", row.schema_version())?;
        output.write_all(b",")?;
        write_csv_cell(output, row.dataset().as_str(), safety, true)?;
        output.write_all(b",")?;
        write!(output, "{}", row.id())?;
        output.write_all(b",")?;
        write_csv_json(output, row.source_refs())?;
        for column in &columns {
            output.write_all(b",")?;
            if let Some(value) = row.field(*column) {
                write_csv_value(output, value, safety)?;
            }
        }
        output.write_all(b"\r\n")?;
    }
    Ok(())
}

fn projected_columns(response: &QueryResponse) -> Vec<FieldId> {
    let mut columns = Vec::new();
    let mut seen = BTreeSet::new();
    for field in &response.universe.columns {
        if !matches!(field, FieldId::Id | FieldId::SourceRefs) && seen.insert(*field) {
            columns.push(*field);
        }
    }
    for row in &response.rows {
        for field in row.fields().keys() {
            if seen.insert(*field) {
                columns.push(*field);
            }
        }
    }
    columns
}

fn write_csv_value(
    output: &mut dyn Write,
    value: &FieldValue,
    safety: CsvSafety,
) -> io::Result<()> {
    match value {
        FieldValue::Null => Ok(()),
        FieldValue::Bool(value) => write!(output, "{value}"),
        FieldValue::Unsigned(value) => write!(output, "{value}"),
        FieldValue::Integer(value) => write!(output, "{value}"),
        FieldValue::Float(value) => write!(output, "{value}"),
        FieldValue::String(value) => write_csv_cell(output, value, safety, true),
        FieldValue::Timestamp(value) => {
            let value = value
                .to_wire()
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
            write_csv_cell(output, &value, safety, true)
        }
        FieldValue::Id(value) => write!(output, "{value}"),
        FieldValue::IdList(value) => write_csv_json(output, value),
        FieldValue::Strings(value) => write_csv_json(output, value),
        FieldValue::Structured(value) => write_csv_json(output, value),
    }
}

fn write_csv_cell(
    output: &mut dyn Write,
    value: &str,
    safety: CsvSafety,
    string_cell: bool,
) -> io::Result<()> {
    let dangerous = string_cell && safety == CsvSafety::Spreadsheet && spreadsheet_danger(value);
    let quoted = value
        .bytes()
        .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'));
    if quoted {
        output.write_all(b"\"")?;
    }
    if dangerous {
        output.write_all(b"'")?;
    }
    let mut start = 0;
    for (index, byte) in value.bytes().enumerate() {
        if byte == b'"' {
            output.write_all(&value.as_bytes()[start..index])?;
            output.write_all(b"\"\"")?;
            start = index + 1;
        }
    }
    output.write_all(&value.as_bytes()[start..])?;
    if quoted {
        output.write_all(b"\"")?;
    }
    Ok(())
}

fn write_csv_json<T: Serialize + ?Sized>(output: &mut dyn Write, value: &T) -> io::Result<()> {
    output.write_all(b"\"")?;
    serde_json::to_writer(CsvQuoted(output), value).map_err(json_error)?;
    output.write_all(b"\"")
}

struct CsvQuoted<'a>(&'a mut dyn Write);

impl Write for CsvQuoted<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut start = 0;
        for (index, byte) in bytes.iter().copied().enumerate() {
            if byte == b'"' {
                self.0.write_all(&bytes[start..index])?;
                self.0.write_all(b"\"\"")?;
                start = index + 1;
            }
        }
        self.0.write_all(&bytes[start..])?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

fn spreadsheet_danger(value: &str) -> bool {
    matches!(value.as_bytes().first(), Some(b'\t' | b'\r'))
        || matches!(
            value.chars().find(|character| !character.is_whitespace()),
            Some('=' | '+' | '-' | '@')
        )
}

fn encode_text(response: &QueryResponse, output: &mut dyn Write) -> io::Result<()> {
    for (index, row) in response.rows.iter().enumerate() {
        if index != 0 {
            output.write_all(b"\n")?;
        }
        writeln!(
            output,
            "Row {}: {} {}",
            index + 1,
            row.dataset(),
            row.id()
        )?;
        if !row.source_refs().is_empty() {
            output.write_all(b"source_refs: ")?;
            write_human_json(output, row.source_refs(), EscapeStyle::Text)?;
            output.write_all(b"\n")?;
        }
        for (field, value) in row.fields() {
            write!(output, "{field}: ")?;
            write_human_value(output, value, EscapeStyle::Text)?;
            output.write_all(b"\n")?;
        }
    }
    Ok(())
}

fn encode_markdown(response: &QueryResponse, output: &mut dyn Write) -> io::Result<()> {
    for (index, row) in response.rows.iter().enumerate() {
        if index != 0 {
            output.write_all(b"\n")?;
        }
        writeln!(output, "## {} {}", row.dataset(), row.id())?;
        if !row.source_refs().is_empty() {
            output.write_all(b"- **source_refs**: ")?;
            write_human_json(output, row.source_refs(), EscapeStyle::Markdown)?;
            output.write_all(b"\n")?;
        }
        for (field, value) in row.fields() {
            write!(output, "- **{field}**: ")?;
            write_human_value(output, value, EscapeStyle::Markdown)?;
            output.write_all(b"\n")?;
        }
    }
    Ok(())
}

fn encode_table(response: &QueryResponse, output: &mut dyn Write) -> io::Result<()> {
    let columns = projected_columns(response);
    output.write_all(b"schema_version | dataset | id | source_refs")?;
    for column in &columns {
        write!(output, " | {column}")?;
    }
    output.write_all(b"\n")?;
    output.write_all(b"--- | --- | --- | ---")?;
    for _ in &columns {
        output.write_all(b" | ---")?;
    }
    output.write_all(b"\n")?;
    for row in &response.rows {
        write!(
            output,
            "{} | {} | {} | ",
            row.schema_version(),
            row.dataset(),
            row.id()
        )?;
        write_human_json(output, row.source_refs(), EscapeStyle::Table)?;
        for column in &columns {
            output.write_all(b" | ")?;
            if let Some(value) = row.field(*column) {
                write_human_value(output, value, EscapeStyle::Table)?;
            }
        }
        output.write_all(b"\n")?;
    }
    Ok(())
}

fn write_human_value(
    output: &mut dyn Write,
    value: &FieldValue,
    style: EscapeStyle,
) -> io::Result<()> {
    match value {
        FieldValue::Null => output.write_all(b"null"),
        FieldValue::Bool(value) => write!(output, "{value}"),
        FieldValue::Unsigned(value) => write!(output, "{value}"),
        FieldValue::Integer(value) => write!(output, "{value}"),
        FieldValue::Float(value) => write!(output, "{value}"),
        FieldValue::String(value) => write_escaped(output, value, style),
        FieldValue::Timestamp(value) => {
            let value = value
                .to_wire()
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
            write_escaped(output, &value, style)
        }
        FieldValue::Id(value) => write!(output, "{value}"),
        FieldValue::IdList(_) | FieldValue::Strings(_) | FieldValue::Structured(_) => {
            write_human_json(output, &WireValue(value), style)
        }
    }
}

fn write_human_json<T: Serialize + ?Sized>(
    output: &mut dyn Write,
    value: &T,
    style: EscapeStyle,
) -> io::Result<()> {
    serde_json::to_writer(EscapingWriter { output, style }, value).map_err(json_error)
}

fn write_escaped(output: &mut dyn Write, value: &str, style: EscapeStyle) -> io::Result<()> {
    EscapingWriter { output, style }.write_all(value.as_bytes())
}

#[derive(Clone, Copy)]
enum EscapeStyle {
    Text,
    Markdown,
    Table,
}

struct EscapingWriter<'a> {
    output: &'a mut dyn Write,
    style: EscapeStyle,
}

impl Write for EscapingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let value = std::str::from_utf8(bytes)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        for character in value.chars() {
            if character.is_control() {
                if matches!(self.style, EscapeStyle::Markdown) {
                    self.output.write_all(b"\\\\")?;
                } else {
                    self.output.write_all(b"\\")?;
                }
                match character {
                    '\n' => self.output.write_all(b"n")?,
                    '\r' => self.output.write_all(b"r")?,
                    '\t' => self.output.write_all(b"t")?,
                    character => write!(self.output, "u{:04x}", u32::from(character))?,
                }
                continue;
            }
            match (self.style, character) {
                (EscapeStyle::Markdown, '&') => self.output.write_all(b"&amp;")?,
                (EscapeStyle::Markdown, '<') => self.output.write_all(b"&lt;")?,
                (EscapeStyle::Markdown, '>') => self.output.write_all(b"&gt;")?,
                (EscapeStyle::Markdown, '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']'
                    | '(' | ')' | '#' | '+' | '-' | '.' | '!' | '|') => {
                    self.output.write_all(b"\\")?;
                    let mut encoded = [0; 4];
                    self.output
                        .write_all(character.encode_utf8(&mut encoded).as_bytes())?;
                }
                (EscapeStyle::Table, '|') => self.output.write_all(b"\\|")?,
                _ => {
                    let mut encoded = [0; 4];
                    self.output
                        .write_all(character.encode_utf8(&mut encoded).as_bytes())?;
                }
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

fn json_error(error: serde_json::Error) -> io::Error {
    match error.io_error_kind() {
        Some(kind) => io::Error::from(kind),
        None => io::Error::from(io::ErrorKind::InvalidData),
    }
}
