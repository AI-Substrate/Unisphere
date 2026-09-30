-- Tool loops: the same tool and input hash repeated at least 3 times in one turn.
SELECT u.source, coalesce(ss.seat_hint, ss.session_id, u.source) AS seat, u.turn_no, u.name,
  u.input_hash, count(*) AS repeats,
  count(*) FILTER (WHERE u.outcome = 'error') AS errors,
  sum(u.result_bytes) AS result_bytes,
  strftime(to_timestamp(min(u.ts_ms) / 1000.0), '%Y-%m-%d %H:%M:%S') AS first_ts,
  round((max(u.ts_ms) - min(u.ts_ms)) / 1000.0, 1) AS span_s,
  min(u.native_offset) AS first_offset
FROM tool_uses_v u
LEFT JOIN sessions_v ss ON ss.source = u.source
WHERE u.input_hash IS NOT NULL
GROUP BY ALL
HAVING count(*) >= 3
ORDER BY repeats DESC, u.source, u.turn_no, u.name
