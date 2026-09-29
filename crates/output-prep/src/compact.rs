//! Compaction rules: keep each source's current generation and merge the
//! sightings `views.sql` merges, so every canonical view reads the same rows
//! before and after a compaction.

use std::collections::{BTreeMap, HashMap, hash_map::Entry};

use unisphere_core::prep::{CallSighting, PrepCallRow, PrepToolUseRow, ToolSighting};

/// Position of a row: rank of its part in file-name order (the order
/// `views.sql` uses) and its row number inside the part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Pos {
    pub part: usize,
    pub row: usize,
}

/// Current generation of every source in the published state.
pub(crate) type Current<'a> = BTreeMap<&'a str, u32>;

fn is_current(current: &Current<'_>, source: &str, generation: u32) -> bool {
    current.get(source) == Some(&generation)
}

/// `views.sql` native order: `coalesce(native_offset, -1), filename, file_row_number`.
fn native_order(offset: Option<u64>, pos: Pos) -> (i64, Pos) {
    (
        offset.map_or(-1, |o| i64::try_from(o).unwrap_or(i64::MAX)),
        pos,
    )
}

/// Rows of the current generation, in their original order.
pub(crate) fn current_rows<T>(
    rows: Vec<(T, Pos)>,
    current: &Current<'_>,
    key: impl Fn(&T) -> (&str, u32),
) -> Vec<T> {
    rows.into_iter()
        .filter(|(row, _)| {
            let (source, generation) = key(row);
            is_current(current, source, generation)
        })
        .map(|(row, _)| row)
        .collect()
}

fn max(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// One row per call: the first sighting's fields, per-field maximum counters,
/// the last non-null stop reason and the summed record count. A call with
/// neither id is never merged.
pub(crate) fn calls(rows: Vec<(PrepCallRow, Pos)>, current: &Current<'_>) -> Vec<PrepCallRow> {
    let mut rows: Vec<_> = rows
        .into_iter()
        .filter(|(r, _)| is_current(current, &r.source, r.generation))
        .collect();
    rows.sort_by_key(|(r, pos)| native_order(r.native_offset, *pos));
    let mut index: HashMap<(String, u32, Option<String>, Option<String>), usize> = HashMap::new();
    let mut out: Vec<PrepCallRow> = Vec::with_capacity(rows.len());
    for (mut row, _) in rows {
        if row.msg_id.is_none() && row.request_id.is_none() {
            row.sighting = CallSighting::First;
            out.push(row);
            continue;
        }
        let key = (
            row.source.clone(),
            row.generation,
            row.msg_id.clone(),
            row.request_id.clone(),
        );
        match index.entry(key) {
            Entry::Occupied(slot) => {
                let merged = &mut out[*slot.get()];
                merged.input = max(merged.input, row.input);
                merged.cw_1h = max(merged.cw_1h, row.cw_1h);
                merged.cw_5m = max(merged.cw_5m, row.cw_5m);
                merged.cache_read = max(merged.cache_read, row.cache_read);
                merged.output = max(merged.output, row.output);
                if row.stop_reason.is_some() {
                    merged.stop_reason = row.stop_reason;
                }
                merged.records += row.records;
            }
            Entry::Vacant(slot) => {
                slot.insert(out.len());
                row.sighting = CallSighting::First;
                out.push(row);
            }
        }
    }
    out
}

/// One row per tool use: every field is the first non-null value over the
/// use sightings, then the result sightings, each in native order. A row
/// without a tool-use id is never merged.
pub(crate) fn tool_uses(
    rows: Vec<(PrepToolUseRow, Pos)>,
    current: &Current<'_>,
) -> Vec<PrepToolUseRow> {
    let mut rows: Vec<_> = rows
        .into_iter()
        .filter(|(r, _)| is_current(current, &r.source, r.generation))
        .collect();
    rows.sort_by_key(|(r, pos)| native_order(r.native_offset, *pos));
    let mut index: HashMap<(String, u32, String), usize> = HashMap::new();
    let mut groups: Vec<Vec<PrepToolUseRow>> = Vec::new();
    for (row, _) in rows {
        let Some(id) = row.tool_use_id.clone() else {
            groups.push(vec![row]);
            continue;
        };
        match index.entry((row.source.clone(), row.generation, id)) {
            Entry::Occupied(slot) => groups[*slot.get()].push(row),
            Entry::Vacant(slot) => {
                slot.insert(groups.len());
                groups.push(vec![row]);
            }
        }
    }
    groups
        .into_iter()
        .map(|mut group| {
            // Stable: native order is kept within use and within result sightings.
            group.sort_by_key(|r| r.sighting != ToolSighting::Use);
            let mut rest = group.into_iter();
            let mut merged = rest.next().expect("groups are never empty");
            for row in rest {
                macro_rules! fill {
                    ($($field:ident),*) => {$(
                        if merged.$field.is_none() {
                            merged.$field = row.$field.clone();
                        }
                    )*};
                }
                fill!(
                    native_offset,
                    native_key,
                    call_msg_id,
                    ts,
                    ts_ms,
                    name,
                    family,
                    input_hash,
                    input_bytes,
                    result_offset,
                    result_bytes,
                    outcome,
                    duration_ms,
                    turn_no
                );
            }
            merged
        })
        .collect()
}
