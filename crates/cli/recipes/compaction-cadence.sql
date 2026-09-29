-- Compaction cadence per session: how often, how large, how much context comes back.
WITH spans AS (
  SELECT c.source, c.trigger, c.pre_tokens, c.first_context_after,
    (c.ts_ms - coalesce(lag(c.ts_ms) OVER (PARTITION BY c.source ORDER BY c.ts_ms), ss.first_event_ms))
      / 3600000.0 AS hours_since_previous
  FROM compactions_v c
  LEFT JOIN sessions_v ss ON ss.source = c.source
  WHERE c.ts_ms IS NOT NULL
)
SELECT s.harness, coalesce(ss.seat_hint, ss.session_id, spans.source) AS seat, spans.source,
  count(*) AS compactions,
  count(*) FILTER (WHERE trigger = 'manual') AS manual,
  count(*) FILTER (WHERE trigger = 'auto') AS auto,
  round(median(hours_since_previous), 2) AS median_hours_between,
  round(min(hours_since_previous), 2) AS min_hours_between,
  median(pre_tokens) AS median_pre_tokens,
  median(first_context_after) AS median_context_after
FROM spans
JOIN sources_v s ON s.source = spans.source
LEFT JOIN sessions_v ss ON ss.source = spans.source
GROUP BY ALL
ORDER BY compactions DESC, median_hours_between, spans.source
