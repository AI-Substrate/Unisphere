-- Every compaction: trigger, size, duration, idle gap and the context rebuilt after it.
SELECT c.source, ss.session_id, c.ts, c.trigger, c.pre_tokens, c.post_tokens, c.duration_ms,
  round(c.gap_ms / 1000.0, 1) AS gap_s_since_last_call, c.first_context_after
FROM compactions_v c
LEFT JOIN sessions_v ss ON ss.source = c.source
ORDER BY c.ts_ms NULLS LAST, c.source, c.native_offset
