-- Main sessions ordered by how long they have been idle (last native event before now).
SELECT coalesce(ss.seat_hint, ss.session_id, ss.source) AS seat, s.harness, s.project, ss.session_id,
  ss.last_event_ts, round((epoch_ms(now()) - ss.last_event_ms) / 3600000.0, 1) AS idle_hours,
  ss.calls, ss.turns, ss.latest_context_total, ss.latest_model, ss.source
FROM sessions_v ss
JOIN sources_v s ON s.source = ss.source
WHERE NOT s.is_sub AND ss.last_event_ms IS NOT NULL
ORDER BY ss.last_event_ms, ss.source
