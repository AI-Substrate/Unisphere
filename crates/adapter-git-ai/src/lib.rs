//! Independent authorship/3.0.0 input-format projection. No upstream implementation is imported.
//! Attribution is not an execution trace, conversation archive or token ledger.
#![forbid(unsafe_code)]
mod query;

pub use query::QUERY_POLICY_VERSION;

use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value, json};
use std::{borrow::Cow, collections::BTreeMap, fmt};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, GitNoteAdapter, GitNotesError, GitNotesLimits,
    LoadedGitNote, MappingOptions, TelemetryRecord,
};

pub const DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "git-ai",
    application: "Git AI",
    description: "Independently read explicit local commit-attached authorship notes using standard Git; Git AI is not required. Attribution, not complete conversations.",
    locations: &[],
    capabilities: AdapterCapabilities {
        export_platforms: &["unix"],
        output_formats: &["otlp-jsonl"],
        sdk_caller_owned_cursor: false,
        cursor_source_assumption: "pinned_git_notes_ref",
        cli_persisted_resume: false,
        delayed_revision_reconciliation: false,
        lossless_archive: false,
    },
};
#[derive(Debug, Clone, Copy, Default)]
pub struct GitAiAdapter;
impl GitNoteAdapter for GitAiAdapter {
    fn name(&self) -> &'static str {
        DESCRIPTOR.id
    }

    fn map_note(
        &self,
        note: &LoadedGitNote,
        options: MappingOptions,
        limits: GitNotesLimits,
    ) -> Result<Vec<TelemetryRecord>, GitNotesError> {
        note.source.validate()?;
        limits.validate()?;
        if note.bytes.len() > limits.max_note_bytes {
            return Err(GitNotesError::NoteLimit);
        }
        let classified = classify_note(
            &note.bytes,
            limits.max_records,
            unisphere_core::MAX_OUTPUT_BATCH_BYTES,
        )
        .map_err(|error| {
            if error == GitNotesError::BatchLimit {
                unisphere_core::PipelineError::new(
                    unisphere_core::PipelineErrorKind::OutputLimit,
                    None,
                )
                .into()
            } else {
                error
            }
        })?;
        let metadata = &classified.metadata;
        let mut records = Vec::new();
        let mut payload_bytes = 0;
        let mut attrs = common(note, "note_metadata", "$note")?;
        for key in ["schema_version", "git_ai_version", "base_commit_sha"] {
            copy_field(&mut attrs, metadata, key);
        }
        push(
            &mut records,
            "unisphere.git_ai.note",
            attrs,
            limits,
            &mut payload_bytes,
        )?;
        for map_name in ["prompts", "sessions", "humans"] {
            if let Some(map) = identity_map(metadata, map_name)? {
                for (key, value) in map {
                    let mut attrs = common(
                        note,
                        "declared_identity",
                        &format!("metadata/{map_name}/{key}"),
                    )?;
                    attrs.insert("unisphere.git_ai.identity.key".into(), json!(key));
                    attrs.insert(
                        "unisphere.git_ai.identity.kind".into(),
                        json!(kind(map_name)),
                    );
                    identity_attributes(
                        &mut attrs,
                        value.as_object().ok_or(GitNotesError::InvalidData)?,
                        options,
                    );
                    push(
                        &mut records,
                        "unisphere.git_ai.identity",
                        attrs,
                        limits,
                        &mut payload_bytes,
                    )?;
                }
            }
        }
        for attribution in &classified.attributions {
            let identity = classified.identity(attribution.map_name, &attribution.identity_key);
            let mut attrs = common(note, "line_attribution", &attribution.subrecord)?;
            attrs.insert(
                "unisphere.git_ai.file.path".into(),
                json!(attribution.file.as_str()),
            );
            attrs.insert(
                "unisphere.git_ai.attestation.key".into(),
                json!(attribution.native_key.as_str()),
            );
            attrs.insert(
                "unisphere.git_ai.identity.key".into(),
                json!(attribution.identity_key.as_str()),
            );
            attrs.insert(
                "unisphere.git_ai.identity.kind".into(),
                json!(kind(attribution.map_name)),
            );
            attrs.insert(
                "unisphere.git_ai.identity_resolution".into(),
                json!(if identity.is_some() {
                    "resolved"
                } else {
                    "unresolved"
                }),
            );
            attrs.insert(
                "unisphere.git_ai.line.start".into(),
                json!(attribution.start),
            );
            attrs.insert("unisphere.git_ai.line.end".into(), json!(attribution.end));
            if let Some(checkpoint) = &attribution.checkpoint {
                attrs.insert(
                    "unisphere.git_ai.session.id".into(),
                    json!(attribution.identity_key.as_str()),
                );
                attrs.insert("unisphere.git_ai.checkpoint.id".into(), json!(checkpoint));
            }
            // Content is retained once on the declared identity, not copied per line range.
            if let Some(identity) = identity {
                identity_attributes(&mut attrs, identity, MappingOptions::default());
            }
            push(
                &mut records,
                "unisphere.git_ai.attribution",
                attrs,
                limits,
                &mut payload_bytes,
            )?;
        }
        Ok(records)
    }
}

pub(crate) struct ClassifiedNote {
    pub(crate) metadata: Map<String, Value>,
    pub(crate) attributions: Vec<NativeAttribution>,
}

impl ClassifiedNote {
    pub(crate) fn identity(&self, map_name: &str, key: &str) -> Option<&Map<String, Value>> {
        self.metadata
            .get(map_name)
            .and_then(Value::as_object)
            .and_then(|map| map.get(key))
            .and_then(Value::as_object)
    }
}

pub(crate) struct NativeAttribution {
    pub(crate) subrecord: String,
    pub(crate) file: String,
    pub(crate) native_key: String,
    pub(crate) map_name: &'static str,
    pub(crate) identity_key: String,
    pub(crate) checkpoint: Option<String>,
    pub(crate) start: u64,
    pub(crate) end: u64,
}

pub(crate) fn classify_note(
    bytes: &[u8],
    max_records: usize,
    max_retained: usize,
) -> Result<ClassifiedNote, GitNotesError> {
    let text = std::str::from_utf8(bytes).map_err(|_| GitNotesError::InvalidData)?;
    let mut offset = 0;
    let mut split = None;
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content == "---" {
            split = Some((offset, offset + line.len()));
            break;
        }
        offset += line.len();
    }
    let (attestations_end, metadata_start) = split.ok_or(GitNotesError::InvalidData)?;
    let NoDuplicates(value) =
        serde_json::from_str(&text[metadata_start..]).map_err(|_| GitNotesError::InvalidData)?;
    let metadata = match value {
        Value::Object(metadata) => metadata,
        _ => return Err(GitNotesError::InvalidData),
    };
    allowed(
        &metadata,
        &[
            "schema_version",
            "git_ai_version",
            "base_commit_sha",
            "prompts",
            "sessions",
            "humans",
        ],
    )?;
    if metadata.get("schema_version").and_then(Value::as_str) != Some("authorship/3.0.0") {
        return Err(GitNotesError::UnsupportedFormat);
    }
    for key in ["git_ai_version", "base_commit_sha"] {
        string_field(&metadata, key)?;
    }
    for map_name in ["prompts", "sessions", "humans"] {
        if let Some(map) = identity_map(&metadata, map_name)? {
            for (key, value) in map {
                validate_identity(map_name, key, value)?;
            }
        }
    }

    let mut count = 1usize;
    for name in ["prompts", "sessions", "humans"] {
        count = count
            .checked_add(identity_map(&metadata, name)?.map_or(0, Map::len))
            .ok_or(GitNotesError::RecordLimit)?;
    }
    if count > max_records {
        return Err(GitNotesError::RecordLimit);
    }
    let mut retained = 0usize;
    let mut attributions = Vec::new();
    let mut file: Option<String> = None;
    let mut file_index = 0usize;
    let mut entry_index = 0usize;
    for line in text[..attestations_end].lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(entry) = line.strip_prefix("  ") {
            let path = file.as_ref().ok_or(GitNotesError::InvalidData)?;
            let (key, ranges) = entry.split_once(' ').ok_or(GitNotesError::InvalidData)?;
            let (map_name, identity_key, checkpoint) = attestation_key(key)?;
            let mut previous_end = 0u64;
            for (range_index, range) in ranges.split(',').enumerate() {
                let (start, end) = line_range(range)?;
                if start <= previous_end {
                    return Err(GitNotesError::InvalidData);
                }
                previous_end = end;
                if count == max_records {
                    return Err(GitNotesError::RecordLimit);
                }
                count += 1;
                let subrecord = format!("attestations/{file_index}/{entry_index}/{range_index}");
                retained = [
                    path.len(),
                    key.len(),
                    identity_key.len(),
                    checkpoint.map_or(0, str::len),
                    subrecord.len(),
                ]
                .into_iter()
                .try_fold(retained, |sum, length| sum.checked_add(length))
                .ok_or(GitNotesError::BatchLimit)?;
                if retained > max_retained {
                    return Err(GitNotesError::BatchLimit);
                }
                attributions.push(NativeAttribution {
                    subrecord,
                    file: path.clone(),
                    native_key: key.to_owned(),
                    map_name,
                    identity_key: identity_key.to_owned(),
                    checkpoint: checkpoint.map(str::to_owned),
                    start,
                    end,
                });
            }
            entry_index += 1;
        } else {
            if file.is_some() && entry_index == 0 {
                return Err(GitNotesError::InvalidData);
            }
            if file.is_some() {
                file_index += 1;
            }
            file = Some(file_path(line)?.into_owned());
            entry_index = 0;
        }
    }
    if file.is_some() && entry_index == 0 {
        return Err(GitNotesError::InvalidData);
    }
    Ok(ClassifiedNote {
        metadata,
        attributions,
    })
}
fn push(
    records: &mut Vec<TelemetryRecord>,
    event: &str,
    attributes: BTreeMap<String, Value>,
    limits: GitNotesLimits,
    payload_bytes: &mut usize,
) -> Result<(), GitNotesError> {
    if records.len() >= limits.max_records {
        return Err(GitNotesError::RecordLimit);
    }
    let record = TelemetryRecord {
        event_name: event.into(),
        timestamp_unix_nano: None,
        attributes,
        body: None,
    };
    unisphere_core::account_git_record(&record, payload_bytes)?;
    records.push(record);
    Ok(())
}
fn common(
    note: &LoadedGitNote,
    kind: &str,
    key: &str,
) -> Result<BTreeMap<String, Value>, GitNotesError> {
    let source = &note.source;
    Ok(BTreeMap::from([
        ("unisphere.profile.version".into(), json!(1)),
        ("unisphere.source.adapter".into(), json!(DESCRIPTOR.id)),
        (
            "unisphere.source.path".into(),
            json!(
                source
                    .repository
                    .to_str()
                    .ok_or(GitNotesError::InvalidData)?
            ),
        ),
        ("unisphere.source.kind".into(), json!(kind)),
        ("unisphere.source.key".into(), json!(key)),
        ("unisphere.source.revision".into(), json!(source.note_blob)),
        ("unisphere.source.format".into(), json!("git_notes")),
        (
            "unisphere.git.repository.id".into(),
            json!(
                source
                    .repository_id
                    .to_str()
                    .ok_or(GitNotesError::InvalidData)?
            ),
        ),
        ("unisphere.git.notes.ref".into(), json!(source.notes_ref)),
        ("unisphere.git.notes.tip".into(), json!(source.notes_tip)),
        ("unisphere.git.commit".into(), json!(source.target_commit)),
        ("unisphere.git.note.blob".into(), json!(source.note_blob)),
    ]))
}
fn allowed(object: &Map<String, Value>, keys: &[&str]) -> Result<(), GitNotesError> {
    if object.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(GitNotesError::UnsupportedFormat);
    }
    Ok(())
}
fn string_field(object: &Map<String, Value>, key: &str) -> Result<(), GitNotesError> {
    if object
        .get(key)
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err(GitNotesError::InvalidData);
    }
    Ok(())
}
fn identity_map<'a>(
    metadata: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a Map<String, Value>>, GitNotesError> {
    match metadata.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        _ => Err(GitNotesError::InvalidData),
    }
}
fn hex(key: &str, length: usize) -> bool {
    key.len() == length && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn prefixed(key: &str, prefix: &str) -> bool {
    key.strip_prefix(prefix)
        .is_some_and(|suffix| hex(suffix, 14))
}
fn legacy(key: &str) -> bool {
    hex(key, 16) || hex(key, 7)
}
fn kind(map_name: &str) -> &'static str {
    match map_name {
        "sessions" => "session",
        "humans" => "human",
        _ => "prompt",
    }
}
fn validate_identity(map_name: &str, key: &str, value: &Value) -> Result<(), GitNotesError> {
    let valid_key = match map_name {
        "sessions" => prefixed(key, "s_"),
        "humans" => prefixed(key, "h_"),
        _ => legacy(key),
    };
    if !valid_key {
        return Err(GitNotesError::UnsupportedFormat);
    }
    let record = value.as_object().ok_or(GitNotesError::InvalidData)?;
    match map_name {
        "sessions" => allowed(record, &["agent_id", "human_author", "custom_attributes"]),
        "humans" => allowed(record, &["author"]),
        _ => allowed(
            record,
            &[
                "agent_id",
                "human_author",
                "custom_attributes",
                "messages",
                "messages_url",
                "total_additions",
                "total_deletions",
                "accepted_lines",
                "overriden_lines",
            ],
        ),
    }?;
    for key in ["author", "human_author", "messages_url"] {
        string_field(record, key)?;
    }
    if let Some(agent) = record.get("agent_id").filter(|value| !value.is_null()) {
        let agent = agent.as_object().ok_or(GitNotesError::InvalidData)?;
        allowed(agent, &["tool", "id", "model"])?;
        for key in ["tool", "id", "model"] {
            string_field(agent, key)?;
        }
    }
    if record
        .get("messages")
        .is_some_and(|value| !value.is_null() && !value.is_array())
        || record
            .get("custom_attributes")
            .is_some_and(|value| !value.is_null() && !value.is_object())
    {
        return Err(GitNotesError::InvalidData);
    }
    for key in [
        "total_additions",
        "total_deletions",
        "accepted_lines",
        "overriden_lines",
    ] {
        if record
            .get(key)
            .is_some_and(|value| !value.is_null() && value.as_i64().is_none_or(|n| n < 0))
        {
            return Err(GitNotesError::InvalidData);
        }
    }
    Ok(())
}
fn copy_field(attributes: &mut BTreeMap<String, Value>, source: &Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        attributes.insert(format!("unisphere.git_ai.{key}"), value.clone());
    }
}
fn identity_attributes(
    attrs: &mut BTreeMap<String, Value>,
    record: &Map<String, Value>,
    options: MappingOptions,
) {
    if let Some(agent) = record.get("agent_id") {
        if let Some(agent) = agent.as_object() {
            for key in ["tool", "id", "model"] {
                if let Some(value) = agent.get(key) {
                    attrs.insert(format!("unisphere.git_ai.agent.{key}"), value.clone());
                }
            }
        } else if agent.is_null() {
            attrs.insert("unisphere.git_ai.agent_id".into(), Value::Null);
        }
    }
    for key in [
        "total_additions",
        "total_deletions",
        "accepted_lines",
        "overriden_lines",
    ] {
        copy_field(attrs, record, key);
    }
    if options.include_content {
        for key in [
            "human_author",
            "author",
            "custom_attributes",
            "messages",
            "messages_url",
        ] {
            copy_field(attrs, record, key);
        }
    }
}
fn attestation_key(key: &str) -> Result<(&'static str, &str, Option<&str>), GitNotesError> {
    if key.starts_with("s_") {
        let (session, trace) = key.split_once("::").ok_or(GitNotesError::InvalidData)?;
        if !prefixed(session, "s_") || !prefixed(trace, "t_") {
            return Err(GitNotesError::UnsupportedFormat);
        }
        Ok(("sessions", session, Some(trace)))
    } else if key.starts_with("h_") {
        if !prefixed(key, "h_") {
            return Err(GitNotesError::UnsupportedFormat);
        }
        Ok(("humans", key, None))
    } else if legacy(key) {
        Ok(("prompts", key, None))
    } else {
        Err(GitNotesError::UnsupportedFormat)
    }
}
fn file_path(line: &str) -> Result<Cow<'_, str>, GitNotesError> {
    let path = if line.starts_with('"') {
        Cow::Owned(serde_json::from_str::<String>(line).map_err(|_| GitNotesError::InvalidData)?)
    } else {
        if line.chars().any(char::is_whitespace) {
            return Err(GitNotesError::InvalidData);
        }
        Cow::Borrowed(line)
    };
    if path.is_empty() || path.contains('\0') {
        return Err(GitNotesError::InvalidData);
    }
    Ok(path)
}
fn line_range(range: &str) -> Result<(u64, u64), GitNotesError> {
    let number = |text: &str| -> Result<u64, GitNotesError> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(GitNotesError::InvalidData);
        }
        let value: u64 = text.parse().map_err(|_| GitNotesError::InvalidData)?;
        if value == 0 || value > i64::MAX as u64 {
            return Err(GitNotesError::InvalidData);
        }
        Ok(value)
    };
    let (start, end) = match range.split_once('-') {
        Some((start, end)) => (number(start)?, number(end)?),
        None => {
            let value = number(range)?;
            (value, value)
        }
    };
    if end < start {
        return Err(GitNotesError::InvalidData);
    }
    Ok((start, end))
}

// Duplicate JSON keys would silently overwrite native identity/provenance evidence.
struct NoDuplicates(Value);
impl<'de> Deserialize<'de> for NoDuplicates {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = NoDuplicates;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(NoDuplicates(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(NoDuplicates(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(NoDuplicates(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| NoDuplicates(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(NoDuplicates(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(NoDuplicates(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(NoDuplicates(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(NoDuplicates(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(NoDuplicates(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some((key, NoDuplicates(value))) =
                    map.next_entry::<String, NoDuplicates>()?
                {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate object key"));
                    }
                }
                Ok(NoDuplicates(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}
