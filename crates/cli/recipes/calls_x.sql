calls_x AS (
  -- One row per canonical call with its source, session, turn and derived flags.
  -- context = input + cache reads + cache writes, null when the dialect records none.
  -- cold: context > 20000 and cache writes >= half of it (a rebuilt cache).
  -- pred_cold: gap since the previous call > the cache TTL (300 s sidechain, 3600 s main),
  -- on the reference parser's 0.1 s rounding. gap_s is -1 for a source's first call.
  SELECT b.*,
    coalesce(b.context > 20000 AND 2 * b.cache_write >= b.context, false) AS cold,
    coalesce(b.gap_ms >= 1000 * b.ttl_s + 50, false) AS pred_cold
  FROM (
    SELECT c.*, s.harness, s.project, s.is_sub, s.agent_id, ss.session_id,
      coalesce(ss.seat_hint, ss.session_id, c.source) AS seat,
      t.origin, t.sender,
      CASE WHEN c.input IS NULL AND c.cache_read IS NULL AND c.cw_1h IS NULL AND c.cw_5m IS NULL
        THEN NULL
        ELSE coalesce(c.input, 0) + coalesce(c.cache_read, 0) + coalesce(c.cw_1h, 0) + coalesce(c.cw_5m, 0)
      END AS context,
      coalesce(c.cw_1h, 0) + coalesce(c.cw_5m, 0) AS cache_write,
      CASE WHEN c.gap_ms < 0 THEN -1 ELSE round(c.gap_ms / 1000.0, 1) END AS gap_s,
      coalesce(c.input, 0) + coalesce(c.cache_read, 0) + coalesce(c.cw_1h, 0) + coalesce(c.cw_5m, 0)
        + coalesce(c.output, 0) AS total_tokens,
      CASE WHEN s.is_sub THEN 300 ELSE 3600 END AS ttl_s
    FROM calls_v c
    JOIN sources_v s ON s.source = c.source
    LEFT JOIN sessions_v ss ON ss.source = c.source
    LEFT JOIN turns_v t ON t.source = c.source AND t.generation = c.generation AND t.turn_no = c.turn_no
  ) b
)
