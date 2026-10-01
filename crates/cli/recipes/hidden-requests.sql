-- Requests absent from usage records: recaps and compactions, with context and idle gap.
SELECT e.ts, ss.session_id, CASE e.kind WHEN 'compaction' THEN 'compaction' ELSE 'recap' END AS kind,
  CASE e.kind WHEN 'compaction' THEN e.pre_tokens ELSE e.last_context END AS last_context_or_pre_tokens,
  round(e.gap_ms / 1000.0, 1) AS gap_s, e.source
FROM events_v e
LEFT JOIN sessions_v ss ON ss.source = e.source
WHERE e.kind IN ('recap', 'compaction')
ORDER BY e.ts_ms NULLS LAST, e.source, e.native_offset
