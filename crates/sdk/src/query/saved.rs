use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use unisphere_core::query::{
    Completeness, ContentAccess, Coverage, Dataset, Digest, FieldId, QueryFailure,
    QueryFailureCode, QueryInput, QueryLimits, RecoveryAction, ResultUniverse, SavedFormat,
    Sensitivity, SourceId, UntrustedProjectedRow, UniverseBasis, ViewInputBasis,
};

use super::rows::SavedRow;

pub(crate) struct ParsedSavedInput {
    pub rows: BTreeMap<Dataset, Vec<SavedRow>>,
    pub coverage: Coverage,
    pub input_basis: ViewInputBasis,
    pub fields_by_source: BTreeMap<SourceId, BTreeSet<FieldId>>,
    pub available_fields: BTreeSet<FieldId>,
    pub retained_access: ContentAccess,
    pub source_revisions: BTreeMap<SourceId, String>,
    pub universe_basis: UniverseBasis,
    pub bounded_by_input: bool,
    pub rows_complete: Completeness,
    pub partitions_complete: Completeness,
}

#[derive(Deserialize)]
struct QueryJsonEnvelope {
    ok: bool,
    #[serde(rename = "command")]
    _command: String,
    v: u16,
    data: QueryJsonData,
    #[serde(rename = "next_action")]
    _next_action: serde_json::Value,
}

#[derive(Deserialize)]
struct QueryJsonData {
    schema_version: u16,
    dataset: Dataset,
    rows: Vec<UntrustedProjectedRow>,
    coverage: Coverage,
    universe: ResultUniverse,
    matched: u64,
    emitted: u64,
    next_cursor: Option<String>,
}

pub(crate) fn parse(
    input: &QueryInput,
    limits: &QueryLimits,
) -> Result<ParsedSavedInput, QueryFailure> {
    let QueryInput::Saved { bytes, format } = input else {
        return Err(QueryFailure::invalid_data());
    };
    if bytes.len() > limits.max_total_input_bytes {
        return Err(QueryFailure::limit(
            unisphere_core::query::LimitKind::TotalInputBytes,
        ));
    }
    if bytes.len() > limits.max_source_bytes {
        return Err(QueryFailure::limit(
            unisphere_core::query::LimitKind::SourceBytes,
        ));
    }
    let input_digest = Digest::of_bytes(bytes);
    match format {
        SavedFormat::QueryJsonV1 => parse_json(bytes, *format, input_digest, limits),
        SavedFormat::QueryJsonlV1 => parse_jsonl(bytes, *format, input_digest, limits),
    }
}

fn parse_json(
    bytes: &[u8],
    format: SavedFormat,
    input_digest: Digest,
    limits: &QueryLimits,
) -> Result<ParsedSavedInput, QueryFailure> {
    let envelope: QueryJsonEnvelope = serde_json::from_slice(bytes).map_err(|_| unsupported())?;
    if !envelope.ok || envelope.v != 1 {
        return Err(unsupported());
    }
    let data = envelope.data;
    if data.schema_version != 1
        || data.rows.len() > limits.max_observations_and_rows
        || data.rows.len() as u64 != data.emitted
        || data.next_cursor.as_ref().is_some_and(|cursor| cursor.len() > limits.max_cursor_bytes)
        || data
            .universe
            .columns
            .iter()
            .any(|field| unisphere_core::query::schema(data.dataset).field(*field).is_none())
    {
        return Err(unsupported());
    }
    data.coverage.validate()?;
    data.universe.validate()?;
    if data.rows.iter().any(|row| row.dataset != data.dataset)
        || (data.next_cursor.is_some()
            && data.universe.rows_complete_for_selection == Completeness::Complete)
        || (data.emitted < data.matched
            && data.universe.rows_complete_for_selection == Completeness::Complete)
    {
        return Err(QueryFailure::invalid_data());
    }
    let rows_complete = data.universe.rows_complete_for_selection;
    let partitions_complete = data.universe.partitions_complete;
    let basis = UniverseBasis::SavedSelection;
    let bounded = data.universe.bounded_by_input;
    let declared_fields = data.universe.columns.iter().copied().collect();
    finish(
        data.rows,
        declared_fields,
        data.coverage,
        ViewInputBasis::Saved {
            format,
            input_sha256: input_digest,
            rows_complete,
            partitions_complete,
        },
        basis,
        bounded,
        rows_complete,
        partitions_complete,
    )
}

fn parse_jsonl(
    bytes: &[u8],
    format: SavedFormat,
    input_digest: Digest,
    limits: &QueryLimits,
) -> Result<ParsedSavedInput, QueryFailure> {
    let mut rows = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if line.len() > limits.max_source_bytes {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::SourceBytes,
            ));
        }
        if rows.len() == limits.max_observations_and_rows {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::ObservationsAndRows,
            ));
        }
        rows.push(serde_json::from_slice(line).map_err(|_| unsupported())?);
    }
    finish(
        rows,
        BTreeSet::new(),
        Coverage {
            discovered_sources: 0,
            loaded_sources: 0,
            selected_sources: 0,
            source_read_complete: false,
            ..Coverage::default()
        },
        ViewInputBasis::Saved {
            format,
            input_sha256: input_digest,
            rows_complete: Completeness::Unknown,
            partitions_complete: Completeness::Unknown,
        },
        UniverseBasis::ProvidedRows,
        true,
        Completeness::Unknown,
        Completeness::Unknown,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish(
    untrusted: Vec<UntrustedProjectedRow>,
    mut available_fields: BTreeSet<FieldId>,
    coverage: Coverage,
    input_basis: ViewInputBasis,
    universe_basis: UniverseBasis,
    bounded_by_input: bool,
    rows_complete: Completeness,
    partitions_complete: Completeness,
) -> Result<ParsedSavedInput, QueryFailure> {
    let mut fields_by_source: BTreeMap<SourceId, BTreeSet<FieldId>> = BTreeMap::new();
    let mut source_revisions = BTreeMap::new();
    let mut retained_access = ContentAccess::default();
    let mut seen = BTreeSet::new();
    let permissive = ContentAccess {
        inspect_fields: FieldId::ALL.iter().copied().collect(),
        emit_content: true,
    };
    available_fields.insert(FieldId::Id);
    available_fields.insert(FieldId::SourceRefs);
    let mut rows: BTreeMap<Dataset, Vec<SavedRow>> = BTreeMap::new();
    for row in untrusted {
        let dataset = row.dataset;
        let row = unisphere_core::query::ProjectedRow::from_untrusted(row, &permissive)?;
        if !seen.insert(row.id()) {
            return Err(QueryFailure::invalid_data());
        }
        available_fields.extend(row.fields().keys().copied());
        for field in row.fields().keys() {
            if unisphere_core::query::schema(dataset)
                .field(*field)
                .is_some_and(|schema| schema.sensitivity == Sensitivity::Sensitive)
            {
                retained_access.inspect_fields.insert(*field);
                retained_access.emit_content = true;
            }
        }
        for reference in row.source_refs() {
            if source_revisions
                .insert(reference.source_id(), reference.revision().to_owned())
                .is_some_and(|revision| revision != reference.revision())
            {
                return Err(QueryFailure::invalid_data());
            }
            let fields = fields_by_source.entry(reference.source_id()).or_default();
            fields.insert(FieldId::Id);
            fields.insert(FieldId::SourceRefs);
            fields.extend(row.fields().keys().copied());
        }
        rows.entry(dataset).or_default().push(SavedRow { row });
    }
    Ok(ParsedSavedInput {
        rows,
        coverage,
        input_basis,
        fields_by_source,
        available_fields,
        retained_access,
        source_revisions,
        universe_basis,
        bounded_by_input,
        rows_complete,
        partitions_complete,
    })
}

fn unsupported() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::UnsupportedSchema,
        RecoveryAction::UseCompleteInput,
    )
}
