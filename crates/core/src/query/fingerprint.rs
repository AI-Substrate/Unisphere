use super::{
    AssociationExtent, AssociationObservation, AssociationStatus, AvailabilityIssue, Completeness,
    ContentAccess, Digest, FieldId, NativeSequence, OfflineRef, QueryFailure, QueryRequest,
    QueryScope, SavedFormat, SourceId, SourceReadStatus, SourceSelection, SourceSelector,
};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, PartialEq, Eq)]
pub struct ViewSourceBinding {
    pub source_id: SourceId,
    pub revision: String,
    pub representation: String,
    pub query_policy_version: String,
}
#[derive(Clone, PartialEq, Eq)]
pub struct RetainedCapability {
    pub access: ContentAccess,
    pub fields_by_source: BTreeMap<SourceId, BTreeSet<FieldId>>,
}
#[derive(Clone, PartialEq, Eq)]
pub enum ViewInputBasis {
    LiveNative,
    Saved {
        format: SavedFormat,
        input_sha256: Digest,
        rows_complete: Completeness,
        partitions_complete: Completeness,
    },
}
#[derive(Clone, PartialEq, Eq)]
pub struct SourceReadFacts {
    pub discovered_sources: u64,
    pub loaded_sources: u64,
    pub selected_sources: u64,
    pub source_status: BTreeMap<SourceReadStatus, u64>,
    pub association_status: BTreeMap<AssociationStatus, u64>,
    pub excluded_adapters: Vec<super::AdapterId>,
    pub source_read_complete: bool,
    pub issues: Vec<AvailabilityIssue>,
}
impl SourceReadFacts {
    pub fn from_coverage(coverage: &super::Coverage) -> Self {
        Self {
            discovered_sources: coverage.discovered_sources,
            loaded_sources: coverage.loaded_sources,
            selected_sources: coverage.selected_sources,
            source_status: coverage.source_status.clone(),
            association_status: coverage.association_status.clone(),
            excluded_adapters: coverage.excluded_adapters.clone(),
            source_read_complete: coverage.source_read_complete,
            issues: coverage.issues.clone(),
        }
    }
}

/// The complete, closed C25 preimage. No response, universe, cursor, action, or row selection can enter it.
#[derive(Clone, PartialEq, Eq)]
pub struct ViewDigestBasis {
    pub view_schema_version: u16,
    pub reconstruction_version: u16,
    pub admitted_scope: QueryScope,
    pub admitted_repository_roots: Vec<PathBuf>,
    pub source_selection: SourceSelection,
    pub sources: Vec<ViewSourceBinding>,
    pub selected_associations: Vec<AssociationObservation>,
    pub source_read_facts: SourceReadFacts,
    pub retained: RetainedCapability,
    pub input: ViewInputBasis,
}
impl ViewDigestBasis {
    pub fn digest(&self) -> Result<Digest, QueryFailure> {
        self.source_selection.validate()?;
        if self.view_schema_version == 0 || self.reconstruction_version == 0 {
            return Err(QueryFailure::invalid_data());
        }
        let source_ids: BTreeSet<_> = self.sources.iter().map(|source| source.source_id).collect();
        let retained_ids: BTreeSet<_> = self.retained.fields_by_source.keys().copied().collect();
        if source_ids.len() != self.sources.len()
            || source_ids != retained_ids
            || self.retained.fields_by_source.values().any(|fields| {
                !fields.contains(&FieldId::Id) || !fields.contains(&FieldId::SourceRefs)
            })
        {
            return Err(QueryFailure::invalid_data());
        }
        let mut json = CanonicalJson::new(b"unisphere/query-view/v1\0");
        json.object(|json| {
            json.member("admitted_repository_roots", |json| {
                json.paths(&self.admitted_repository_roots)
            })?;
            json.member("admitted_scope", |json| json.scope(&self.admitted_scope))?;
            json.member("input", |json| json.input(&self.input))?;
            json.member("reconstruction_version", |json| {
                json.integer(self.reconstruction_version)
            })?;
            json.member("retained", |json| json.retained(&self.retained))?;
            json.member("selected_associations", |json| {
                json.associations(&self.selected_associations)
            })?;
            json.member("source_read_facts", |json| {
                json.source_read_facts(&self.source_read_facts)
            })?;
            json.member("source_selection", |json| {
                json.selection(&self.source_selection)
            })?;
            json.member("sources", |json| json.sources(&self.sources))?;
            json.member("view_schema_version", |json| {
                json.integer(self.view_schema_version)
            })
        })?;
        Ok(json.finish())
    }
}

/// Cursor bindings compare request options first so option changes cannot be mislabeled as source mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CursorBinding {
    pub query_digest: Digest,
    pub view_digest: Digest,
}
impl CursorBinding {
    pub fn validate(
        &self,
        expected_query: Digest,
        expected_view: Digest,
    ) -> Result<(), QueryFailure> {
        if self.query_digest != expected_query {
            return Err(QueryFailure::stale_cursor(
                super::CursorMismatchReason::QueryOptionsChanged,
            ));
        }
        if self.view_digest != expected_view {
            return Err(QueryFailure::stale_cursor(
                super::CursorMismatchReason::SourceViewChanged,
            ));
        }
        Ok(())
    }
}

/// Bind normalized row-selection and projection options independently of the source-view digest.
pub fn query_binding_digest(
    request: &QueryRequest,
    selection: &SourceSelection,
) -> Result<Digest, QueryFailure> {
    request.validate()?;
    selection.validate()?;

    #[derive(Serialize)]
    struct NormalizedFilter<'a> {
        field: FieldId,
        predicate: super::Predicate,
        values: Vec<&'a super::FieldValue>,
        ignore_case: bool,
    }
    #[derive(Serialize)]
    struct NormalizedTime<'a> {
        field: Option<FieldId>,
        since: &'a Option<super::Timestamp>,
        until: &'a Option<super::Timestamp>,
        include_undated: bool,
    }
    #[derive(Serialize)]
    struct Basis<'a> {
        allow_partial: bool,
        branch: &'a Option<super::EntityId>,
        columns: &'a [FieldId],
        context: &'a super::ContextWindow,
        dataset: super::Dataset,
        filters: &'a [NormalizedFilter<'a>],
        include_content: bool,
        limit: Option<usize>,
        operation: &'a super::Operation,
        scope: &'a QueryScope,
        selection: &'a SourceSelection,
        sort: &'a [super::SortKey],
        time: NormalizedTime<'a>,
        turn_range: &'a Option<super::InclusiveRange>,
        unresolved: super::UnresolvedPolicy,
    }

    let schema = super::schema(request.dataset);
    let columns = request.columns.as_deref().unwrap_or(schema.default_columns);
    let default_sort: Vec<_> = schema
        .default_order
        .iter()
        .copied()
        .map(|field| super::SortKey {
            field,
            direction: super::SortDirection::Ascending,
        })
        .collect();
    let sort = if request.sort.is_empty() {
        default_sort.as_slice()
    } else {
        request.sort.as_slice()
    };
    let limit = match request.limit {
        Some(0) => None,
        Some(limit) => Some(limit),
        None if request.operation.kind() == super::OperationKind::List => Some(50),
        None => None,
    };
    let mut filter_groups: BTreeMap<(FieldId, super::Predicate, bool), Vec<&super::FieldValue>> =
        BTreeMap::new();
    for filter in &request.filters {
        filter_groups
            .entry((filter.field, filter.predicate, filter.ignore_case))
            .or_default()
            .extend(&filter.values);
    }
    let filters: Vec<_> = filter_groups
        .into_iter()
        .map(|((field, predicate, ignore_case), mut values)| {
            values.sort_by(|left, right| field_value_order(left, right));
            values.dedup_by(|left, right| field_value_order(left, right) == Ordering::Equal);
            NormalizedFilter {
                field,
                predicate,
                values,
                ignore_case,
            }
        })
        .collect();
    let time = NormalizedTime {
        field: request.time.field.or(schema.default_time_field),
        since: &request.time.since,
        until: &request.time.until,
        include_undated: request.time.include_undated,
    };
    let basis = Basis {
        allow_partial: request.allow_partial,
        branch: &request.branch,
        columns,
        context: &request.context,
        dataset: request.dataset,
        filters: &filters,
        include_content: request.include_content,
        limit,
        operation: &request.operation,
        scope: &request.scope,
        selection,
        sort,
        time,
        turn_range: &request.turn_range,
        unresolved: request.unresolved,
    };
    let mut sink = HashSink(Sha256::new());
    sink.0.update(b"unisphere/query-request/v3\0");
    serde_json::to_writer(&mut sink, &basis).map_err(|_| QueryFailure::invalid_data())?;
    Ok(Digest::from_bytes(sink.0.finalize().into()))
}

fn field_value_order(left: &super::FieldValue, right: &super::FieldValue) -> Ordering {
    use super::FieldValue;
    let rank = |value: &FieldValue| match value {
        FieldValue::Null => 0,
        FieldValue::Bool(_) => 1,
        FieldValue::Unsigned(_) => 2,
        FieldValue::Integer(_) => 3,
        FieldValue::Float(_) => 4,
        FieldValue::String(_) => 5,
        FieldValue::Timestamp(_) => 6,
        FieldValue::Id(_) => 7,
        FieldValue::IdList(_) => 8,
        FieldValue::Strings(_) => 9,
        FieldValue::Structured(_) => 10,
    };
    rank(left)
        .cmp(&rank(right))
        .then_with(|| match (left, right) {
            (FieldValue::Null, FieldValue::Null) => Ordering::Equal,
            (FieldValue::Bool(left), FieldValue::Bool(right)) => left.cmp(right),
            (FieldValue::Unsigned(left), FieldValue::Unsigned(right)) => left.cmp(right),
            (FieldValue::Integer(left), FieldValue::Integer(right)) => left.cmp(right),
            (FieldValue::Float(left), FieldValue::Float(right)) => left.total_cmp(right),
            (FieldValue::String(left), FieldValue::String(right)) => left.cmp(right),
            (FieldValue::Timestamp(left), FieldValue::Timestamp(right)) => {
                left.unix_nanos().cmp(&right.unix_nanos())
            }
            (FieldValue::Id(left), FieldValue::Id(right)) => left.cmp(right),
            (FieldValue::IdList(left), FieldValue::IdList(right)) => left.cmp(right),
            (FieldValue::Strings(left), FieldValue::Strings(right)) => left.cmp(right),
            (FieldValue::Structured(left), FieldValue::Structured(right)) => {
                left.to_string().cmp(&right.to_string())
            }
            _ => Ordering::Equal,
        })
}

struct HashSink(Sha256);
impl Write for HashSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct CanonicalJson {
    sink: HashSink,
    first: Vec<bool>,
}
impl CanonicalJson {
    fn new(prefix: &[u8]) -> Self {
        let mut sink = HashSink(Sha256::new());
        sink.0.update(prefix);
        Self {
            sink,
            first: Vec::new(),
        }
    }
    fn finish(self) -> Digest {
        Digest::from_bytes(self.sink.0.finalize().into())
    }
    fn raw(&mut self, value: &str) -> Result<(), QueryFailure> {
        self.sink
            .write_all(value.as_bytes())
            .map_err(|_| QueryFailure::invalid_data())
    }
    fn string(&mut self, value: &str) -> Result<(), QueryFailure> {
        serde_json::to_writer(&mut self.sink, value).map_err(|_| QueryFailure::invalid_data())
    }
    fn integer(&mut self, value: impl std::fmt::Display) -> Result<(), QueryFailure> {
        write!(self.sink, "{value}").map_err(|_| QueryFailure::invalid_data())
    }
    fn boolean(&mut self, value: bool) -> Result<(), QueryFailure> {
        self.raw(if value { "true" } else { "false" })
    }
    fn nullable<T>(
        &mut self,
        value: Option<&T>,
        write: impl FnOnce(&mut Self, &T) -> Result<(), QueryFailure>,
    ) -> Result<(), QueryFailure> {
        match value {
            Some(value) => write(self, value),
            None => self.raw("null"),
        }
    }
    fn object(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<(), QueryFailure>,
    ) -> Result<(), QueryFailure> {
        self.raw("{")?;
        self.first.push(true);
        body(self)?;
        self.first.pop();
        self.raw("}")
    }
    fn array<T>(
        &mut self,
        values: impl IntoIterator<Item = T>,
        mut write: impl FnMut(&mut Self, T) -> Result<(), QueryFailure>,
    ) -> Result<(), QueryFailure> {
        self.raw("[")?;
        let mut first = true;
        for value in values {
            if !first {
                self.raw(",")?;
            }
            first = false;
            write(self, value)?;
        }
        self.raw("]")
    }
    fn member(
        &mut self,
        key: &str,
        value: impl FnOnce(&mut Self) -> Result<(), QueryFailure>,
    ) -> Result<(), QueryFailure> {
        let needs_comma = {
            let first = self
                .first
                .last_mut()
                .ok_or_else(QueryFailure::invalid_data)?;
            let needs_comma = !*first;
            *first = false;
            needs_comma
        };
        if needs_comma {
            self.raw(",")?;
        }
        self.string(key)?;
        self.raw(":")?;
        value(self)
    }
    fn paths(&mut self, paths: &[PathBuf]) -> Result<(), QueryFailure> {
        let mut sorted: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        sorted.sort_by(|a, b| {
            a.as_os_str()
                .as_encoded_bytes()
                .cmp(b.as_os_str().as_encoded_bytes())
        });
        sorted.dedup();
        if sorted.len() != paths.len()
            || sorted
                .iter()
                .any(|path| !path.is_absolute() || path.to_str().is_none())
        {
            return Err(QueryFailure::invalid_data());
        }
        self.array(sorted, |json, path| {
            json.string(path.to_str().expect("validated UTF-8"))
        })
    }
    fn scope(&mut self, scope: &QueryScope) -> Result<(), QueryFailure> {
        self.object(|json| match scope {
            QueryScope::Repository { path, scope } => {
                if !path.is_absolute() {
                    return Err(QueryFailure::invalid_data());
                }
                json.member("kind", |json| json.string("repository"))?;
                json.member("path", |json| {
                    json.string(path.to_str().ok_or_else(QueryFailure::invalid_data)?)
                })?;
                json.member("scope", |json| json.string(scope.as_str()))
            }
            QueryScope::Source { selector } => {
                json.member("kind", |json| json.string("source"))?;
                json.member("selector", |json| json.selector(selector))
            }
            QueryScope::Offline { input } => {
                json.member("input", |json| json.offline(input))?;
                json.member("kind", |json| json.string("offline"))
            }
        })
    }
    fn selector(&mut self, selector: &SourceSelector) -> Result<(), QueryFailure> {
        self.object(|json| match selector {
            SourceSelector::Id(id) => {
                json.member("id", |json| json.string(&id.to_string()))?;
                json.member("kind", |json| json.string("id"))
            }
            SourceSelector::Path { path, adapter } => {
                if !path.is_absolute() {
                    return Err(QueryFailure::invalid_data());
                }
                json.member("adapter", |json| {
                    json.nullable(adapter.as_ref(), |json, id| json.string(id.as_str()))
                })?;
                json.member("kind", |json| json.string("path"))?;
                json.member("path", |json| {
                    json.string(path.to_str().ok_or_else(QueryFailure::invalid_data)?)
                })
            }
        })
    }
    fn offline(&mut self, input: &OfflineRef) -> Result<(), QueryFailure> {
        self.object(|json| match input {
            OfflineRef::File(path) => {
                if !path.is_absolute() {
                    return Err(QueryFailure::invalid_data());
                }
                json.member("kind", |json| json.string("file"))?;
                json.member("path", |json| {
                    json.string(path.to_str().ok_or_else(QueryFailure::invalid_data)?)
                })
            }
            OfflineRef::Stdin => json.member("kind", |json| json.string("stdin")),
        })
    }
    fn selection(&mut self, value: &SourceSelection) -> Result<(), QueryFailure> {
        self.object(|json| {
            json.member("exclude_adapters", |json| {
                json.array(value.exclude_adapters.iter(), |json, id| {
                    json.string(id.as_str())
                })
            })?;
            json.member("exclude_harnesses", |json| {
                json.array(value.exclude_harnesses.iter(), |json, id| {
                    json.string(id.as_str())
                })
            })?;
            json.member("include_adapters", |json| {
                json.array(value.include_adapters.iter(), |json, id| {
                    json.string(id.as_str())
                })
            })?;
            json.member("include_harnesses", |json| {
                json.array(value.include_harnesses.iter(), |json, id| {
                    json.string(id.as_str())
                })
            })
        })
    }
    fn sources(&mut self, values: &[ViewSourceBinding]) -> Result<(), QueryFailure> {
        let mut sorted: Vec<&ViewSourceBinding> = values.iter().collect();
        sorted.sort_by_key(|value| value.source_id);
        if sorted
            .windows(2)
            .any(|pair| pair[0].source_id == pair[1].source_id)
        {
            return Err(QueryFailure::invalid_data());
        }
        self.array(sorted, |json, value| {
            if value.revision.is_empty()
                || value.representation.is_empty()
                || value.query_policy_version.is_empty()
            {
                return Err(QueryFailure::invalid_data());
            }
            json.object(|json| {
                json.member("query_policy_version", |json| {
                    json.string(&value.query_policy_version)
                })?;
                json.member("representation", |json| json.string(&value.representation))?;
                json.member("revision", |json| json.string(&value.revision))?;
                json.member("source_id", |json| {
                    json.string(&value.source_id.to_string())
                })
            })
        })
    }
    fn retained(&mut self, value: &RetainedCapability) -> Result<(), QueryFailure> {
        self.object(|json| {
            json.member("access", |json| {
                json.object(|json| {
                    json.member("emit_content", |json| {
                        json.boolean(value.access.emit_content)
                    })?;
                    json.member("inspect_fields", |json| {
                        json.array(value.access.inspect_fields.iter(), |json, field| {
                            json.string(field.as_str())
                        })
                    })
                })
            })?;
            json.member("fields_by_source", |json| {
                json.object(|json| {
                    for (source, fields) in &value.fields_by_source {
                        json.member(&source.to_string(), |json| {
                            json.array(fields.iter(), |json, field| json.string(field.as_str()))
                        })?;
                    }
                    Ok(())
                })
            })
        })
    }
    fn input(&mut self, value: &ViewInputBasis) -> Result<(), QueryFailure> {
        self.object(|json| match value {
            ViewInputBasis::LiveNative => json.member("kind", |json| json.string("live_native")),
            ViewInputBasis::Saved {
                format,
                input_sha256,
                rows_complete,
                partitions_complete,
            } => {
                json.member("format", |json| json.string(format.as_str()))?;
                json.member("input_sha256", |json| {
                    json.string(&input_sha256.to_string())
                })?;
                json.member("kind", |json| json.string("saved"))?;
                json.member("partitions_complete", |json| {
                    json.string(partitions_complete.as_str())
                })?;
                json.member("rows_complete", |json| json.string(rows_complete.as_str()))
            }
        })
    }
    fn sequence(&mut self, value: &NativeSequence) -> Result<(), QueryFailure> {
        self.object(|json| {
            json.member("key", |json| {
                json.array(value.key.iter(), |json, byte| json.integer(*byte))
            })?;
            json.member("version", |json| json.integer(value.version))
        })
    }
    fn extent(&mut self, value: &AssociationExtent) -> Result<(), QueryFailure> {
        self.object(|json| match value {
            AssociationExtent::Partition => json.member("kind", |json| json.string("partition")),
            AssociationExtent::Record(sequence) => {
                json.member("kind", |json| json.string("record"))?;
                json.member("sequence", |json| json.sequence(sequence))
            }
            AssociationExtent::From { start, until } => {
                json.member("kind", |json| json.string("from"))?;
                json.member("start", |json| json.sequence(start))?;
                json.member("until", |json| {
                    json.nullable(until.as_ref(), |json, sequence| json.sequence(sequence))
                })
            }
        })
    }
    fn associations(&mut self, values: &[AssociationObservation]) -> Result<(), QueryFailure> {
        let mut sorted: Vec<&AssociationObservation> = values.iter().collect();
        sorted.sort_by(compare_association);
        self.array(sorted, |json, value| {
            json.object(|json| {
                json.member("applies_to", |json| json.extent(&value.applies_to))?;
                json.member("basis", |json| json.string(value.basis.as_str()))?;
                json.member("partition", |json| {
                    json.string(&value.partition.to_string())
                })?;
                json.member("path", |json| {
                    json.nullable(value.path.as_ref(), |json, path| {
                        if !path.is_absolute() {
                            return Err(QueryFailure::invalid_data());
                        }
                        json.string(path.to_str().ok_or_else(QueryFailure::invalid_data)?)
                    })
                })
            })
        })
    }
    fn issues(&mut self, values: &[AvailabilityIssue]) -> Result<(), QueryFailure> {
        let mut sorted: Vec<&AvailabilityIssue> = values.iter().collect();
        sorted.sort_by_key(|issue| {
            (
                issue.code,
                issue.field,
                issue.source,
                issue.entity,
                issue.offset,
            )
        });
        self.array(sorted, |json, issue| {
            json.object(|json| {
                json.member("code", |json| json.string(issue.code.as_str()))?;
                json.member("entity", |json| {
                    json.nullable(issue.entity.as_ref(), |json, id| {
                        json.string(&id.to_string())
                    })
                })?;
                json.member("field", |json| {
                    json.nullable(issue.field.as_ref(), |json, field| {
                        json.string(field.as_str())
                    })
                })?;
                json.member("offset", |json| {
                    json.nullable(issue.offset.as_ref(), |json, offset| json.integer(*offset))
                })?;
                json.member("source", |json| {
                    json.nullable(issue.source.as_ref(), |json, id| {
                        json.string(&id.to_string())
                    })
                })
            })
        })
    }
    fn source_read_facts(&mut self, value: &SourceReadFacts) -> Result<(), QueryFailure> {
        let source_status_total = value
            .source_status
            .values()
            .try_fold(0_u64, |sum, count| sum.checked_add(*count))
            .ok_or_else(QueryFailure::invalid_data)?;
        if value.loaded_sources > value.discovered_sources
            || value.selected_sources > value.loaded_sources
            || source_status_total > value.discovered_sources
        {
            return Err(QueryFailure::invalid_data());
        }
        self.object(|json| {
            json.member("association_status", |json| {
                json.object(|json| {
                    for (key, count) in &value.association_status {
                        json.member(key.as_str(), |json| json.integer(*count))?;
                    }
                    Ok(())
                })
            })?;
            json.member("discovered_sources", |json| {
                json.integer(value.discovered_sources)
            })?;
            json.member("excluded_adapters", |json| {
                let mut ids: Vec<_> = value.excluded_adapters.iter().collect();
                ids.sort();
                if ids.windows(2).any(|pair| pair[0] == pair[1]) {
                    return Err(QueryFailure::invalid_data());
                }
                json.array(ids, |json, id| json.string(id.as_str()))
            })?;
            json.member("issues", |json| json.issues(&value.issues))?;
            json.member("loaded_sources", |json| json.integer(value.loaded_sources))?;
            json.member("selected_sources", |json| {
                json.integer(value.selected_sources)
            })?;
            json.member("source_read_complete", |json| {
                json.boolean(value.source_read_complete)
            })?;
            json.member("source_status", |json| {
                json.object(|json| {
                    for (key, count) in &value.source_status {
                        json.member(key.as_str(), |json| json.integer(*count))?;
                    }
                    Ok(())
                })
            })
        })
    }
}

fn compare_association(
    left: &&AssociationObservation,
    right: &&AssociationObservation,
) -> Ordering {
    left.basis
        .cmp(&right.basis)
        .then_with(|| {
            left.path
                .as_ref()
                .map(|path| path.as_os_str().as_encoded_bytes())
                .cmp(
                    &right
                        .path
                        .as_ref()
                        .map(|path| path.as_os_str().as_encoded_bytes()),
                )
        })
        .then_with(|| left.partition.cmp(&right.partition))
        .then_with(|| compare_extent(&left.applies_to, &right.applies_to))
}

fn compare_extent(left: &AssociationExtent, right: &AssociationExtent) -> Ordering {
    match (left, right) {
        (AssociationExtent::Partition, AssociationExtent::Partition) => Ordering::Equal,
        (AssociationExtent::Partition, _) => Ordering::Less,
        (_, AssociationExtent::Partition) => Ordering::Greater,
        (AssociationExtent::Record(left), AssociationExtent::Record(right)) => left.cmp(right),
        (AssociationExtent::Record(_), AssociationExtent::From { .. }) => Ordering::Less,
        (AssociationExtent::From { .. }, AssociationExtent::Record(_)) => Ordering::Greater,
        (
            AssociationExtent::From {
                start: left_start,
                until: left_until,
            },
            AssociationExtent::From {
                start: right_start,
                until: right_until,
            },
        ) => left_start
            .cmp(right_start)
            .then_with(|| left_until.cmp(right_until)),
    }
}
