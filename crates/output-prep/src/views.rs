//! `TARGET/views.sql`: the canonical DuckDB views over the published parts.
//!
//! Generated from the committed part list, so a view never reads a part of an
//! uncommitted run or a part a compaction is about to delete. Every rule here is
//! mirrored by [`crate::compact`]: compaction must not change any view result.

use std::fmt::Write;

use unisphere_core::prep::PREP_TABLE_SCHEMA_VERSION;

/// Canonical view names, in definition order.
pub const VIEWS: [&str; 8] = [
    "sources_v",
    "sessions_v",
    "calls_v",
    "turns_v",
    "triggers_v",
    "events_v",
    "tool_uses_v",
    "compactions_v",
];

/// Keeps rows of each source's current generation (as published in `sources`).
const CURRENT: &str = "WHERE EXISTS (SELECT 1 FROM sources_v s WHERE s.source = p.source AND s.generation = p.generation)";

/// Native order of the sightings of one key: byte offset for append sources,
/// then part and row position (snapshot generations are written by one run).
const NATIVE_ORDER: &str = "coalesce(native_offset, -1), filename, file_row_number";

const TOOL_FIELDS: [&str; 14] = [
    "native_offset",
    "native_key",
    "call_msg_id",
    "ts",
    "ts_ms",
    "name",
    "family",
    "input_hash",
    "input_bytes",
    "result_offset",
    "result_bytes",
    "outcome",
    "duration_ms",
    "turn_no",
];

/// DuckDB source expression reading exactly the committed parts of `table`.
fn parts_of(parts: &[String], table: &str, positions: bool) -> String {
    let prefix = format!("tables/{table}/");
    let mut files: Vec<&str> = parts
        .iter()
        .map(String::as_str)
        .filter(|p| p.starts_with(&prefix))
        .collect();
    files.sort_unstable();
    let list = files
        .iter()
        .map(|f| format!("    '{f}'"))
        .collect::<Vec<_>>()
        .join(",\n");
    let options = if positions {
        ", filename = true, file_row_number = true"
    } else {
        ""
    };
    format!("read_parquet([\n{list}\n  ]{options})")
}

pub(crate) fn views_sql(parts: &[String]) -> String {
    let mut sql = format!(
        "-- Canonical prep views (DuckDB), table schema version {PREP_TABLE_SCHEMA_VERSION}.\n\
         -- Generated from state.json on every publication; do not edit.\n\
         -- Paths are relative to this directory: `cd TARGET && duckdb -init views.sql`,\n\
         -- or `SET file_search_path = 'TARGET';` before `.read TARGET/views.sql`.\n\
         -- *_v views keep only each source's current generation. calls_v merges the\n\
         -- sightings of one (source, generation, msg_id, request_id) call: counters\n\
         -- take the per-field maximum, stop_reason the last non-null value, records\n\
         -- the sum, every other field the first sighting's; a call with neither id\n\
         -- is its own row. tool_uses_v merges the use and result sightings of one\n\
         -- (source, generation, tool_use_id): each field is the first non-null value,\n\
         -- use sightings first. compactions_v adds first_context_after, the context\n\
         -- (input + cache_read + cw_1h + cw_5m) of the first main-chain call after\n\
         -- the boundary in the same source.\n\n"
    );
    let _ = writeln!(
        sql,
        "CREATE OR REPLACE VIEW sources_v AS\nSELECT * FROM read_parquet('tables/sources.parquet');\n"
    );
    let _ = writeln!(
        sql,
        "CREATE OR REPLACE VIEW sessions_v AS\nSELECT * FROM read_parquet('tables/sessions.parquet');\n"
    );

    let call_group = "PARTITION BY source, generation, msg_id, request_id, solo";
    let _ = writeln!(
        sql,
        "CREATE OR REPLACE VIEW calls_v AS\n\
         SELECT * EXCLUDE (sighting, filename, file_row_number, solo, pos) FROM (\n\
         \x20 SELECT * REPLACE (\n\
         \x20     last_value(stop_reason IGNORE NULLS) OVER whole AS stop_reason,\n\
         \x20     max(input) OVER grp AS input,\n\
         \x20     max(cw_1h) OVER grp AS cw_1h,\n\
         \x20     max(cw_5m) OVER grp AS cw_5m,\n\
         \x20     max(cache_read) OVER grp AS cache_read,\n\
         \x20     max(output) OVER grp AS output,\n\
         \x20     CAST(sum(records) OVER grp AS BIGINT) AS records),\n\
         \x20   row_number() OVER ordered AS pos\n\
         \x20 FROM (\n\
         \x20   SELECT p.*, CASE WHEN p.msg_id IS NULL AND p.request_id IS NULL\n\
         \x20     THEN p.filename || '#' || p.file_row_number END AS solo\n\
         \x20   FROM {} p\n\
         \x20   {CURRENT}\n\
         \x20 )\n\
         \x20 WINDOW grp AS ({call_group}),\n\
         \x20   ordered AS ({call_group} ORDER BY {NATIVE_ORDER}),\n\
         \x20   whole AS ({call_group} ORDER BY {NATIVE_ORDER}\n\
         \x20     ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)\n\
         ) WHERE pos = 1;\n",
        parts_of(parts, "calls", true)
    );

    for table in ["turns", "triggers", "events"] {
        let _ = writeln!(
            sql,
            "CREATE OR REPLACE VIEW {table}_v AS\nSELECT p.* FROM {} p\n{CURRENT};\n",
            parts_of(parts, table, false)
        );
    }

    let tool_group = "PARTITION BY source, generation, tool_use_id, solo";
    let tool_order = format!("sighting <> 'use', {NATIVE_ORDER}");
    let firsts = TOOL_FIELDS
        .iter()
        .map(|f| format!("      first_value({f} IGNORE NULLS) OVER whole AS {f}"))
        .collect::<Vec<_>>()
        .join(",\n");
    let _ = writeln!(
        sql,
        "CREATE OR REPLACE VIEW tool_uses_v AS\n\
         SELECT * EXCLUDE (sighting, filename, file_row_number, solo, pos) FROM (\n\
         \x20 SELECT * REPLACE (\n{firsts}),\n\
         \x20   row_number() OVER ordered AS pos\n\
         \x20 FROM (\n\
         \x20   SELECT p.*, CASE WHEN p.tool_use_id IS NULL\n\
         \x20     THEN p.filename || '#' || p.file_row_number END AS solo\n\
         \x20   FROM {} p\n\
         \x20   {CURRENT}\n\
         \x20 )\n\
         \x20 WINDOW ordered AS ({tool_group} ORDER BY {tool_order}),\n\
         \x20   whole AS ({tool_group} ORDER BY {tool_order}\n\
         \x20     ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)\n\
         ) WHERE pos = 1;\n",
        parts_of(parts, "tool_uses", true)
    );

    let _ = writeln!(
        sql,
        "CREATE OR REPLACE VIEW compactions_v AS\n\
         SELECT e.*, (\n\
         \x20 SELECT c.input + c.cache_read + CASE WHEN c.cw_1h IS NULL AND c.cw_5m IS NULL THEN NULL\n\
         \x20     ELSE coalesce(c.cw_1h, 0) + coalesce(c.cw_5m, 0) END\n\
         \x20 FROM calls_v c\n\
         \x20 WHERE c.source = e.source AND c.generation = e.generation AND NOT c.is_sidechain\n\
         \x20   AND CASE WHEN e.native_offset IS NOT NULL AND c.native_offset IS NOT NULL\n\
         \x20         THEN c.native_offset > e.native_offset\n\
         \x20       WHEN e.ts_ms IS NOT NULL AND c.ts_ms IS NOT NULL THEN c.ts_ms > e.ts_ms\n\
         \x20       ELSE false END\n\
         \x20 ORDER BY c.native_offset NULLS LAST, c.ts_ms NULLS LAST, c.native_key NULLS LAST,\n\
         \x20   c.msg_id NULLS LAST, c.request_id NULLS LAST\n\
         \x20 LIMIT 1) AS first_context_after\n\
         FROM events_v e WHERE e.kind = 'compaction';"
    );
    sql
}
