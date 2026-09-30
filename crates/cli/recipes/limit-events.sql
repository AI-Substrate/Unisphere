-- When the fleet was stopped: limit notices and auto-continuations after them.
SELECT ts, session_id, seat, kind, resets_at, resets_at_ms, source FROM (
  SELECT e.ts, e.ts_ms, e.source, e.subkind AS kind, e.resets_at, e.resets_at_ms
  FROM events_v e WHERE e.kind = 'limit_notice'
  UNION ALL
  SELECT t.ts, t.ts_ms, t.source, 'auto_continuation', NULL, NULL
  FROM triggers_v t WHERE t.kind = 'auto-continuation'
) u
LEFT JOIN (SELECT source, session_id, coalesce(seat_hint, session_id) AS seat FROM sessions_v) ss USING (source)
ORDER BY ts_ms NULLS LAST, source, kind
