turns_x AS (
  -- One row per turn with the token classes of its calls and its opening call's facts.
  SELECT t.source, t.generation, t.turn_no, t.started_ts, t.started_ts_ms, t.origin, t.sender,
    t.pij_msg_id, t.body_key, t.opener_offset, t.opener_ts_ms, t.first_call_offset,
    s.harness, s.project, s.is_sub, ss.session_id,
    coalesce(ss.seat_hint, ss.session_id, t.source) AS seat,
    count(c.source) AS calls,
    sum(c.input) AS input, sum(c.cw_1h) AS cw_1h, sum(c.cw_5m) AS cw_5m,
    sum(c.cache_read) AS cache_read, sum(c.output) AS output,
    coalesce(sum(c.total_tokens), 0) AS total_tokens,
    max(c.context) AS max_context,
    any_value(c.context) FILTER (WHERE c.call_in_turn = 1) AS first_call_context,
    any_value(c.gap_s) FILTER (WHERE c.call_in_turn = 1) AS first_call_gap_s,
    coalesce(bool_or(c.cold) FILTER (WHERE c.call_in_turn = 1), false) AS cold,
    coalesce(bool_or(c.cold AND c.pred_cold) FILTER (WHERE c.call_in_turn = 1), false) AS idle_wake,
    coalesce(sum(c.cache_write) FILTER (WHERE c.call_in_turn = 1 AND c.cold), 0) AS cold_write_tokens
  FROM turns_v t
  JOIN sources_v s ON s.source = t.source
  LEFT JOIN sessions_v ss ON ss.source = t.source
  LEFT JOIN calls_x c ON c.source = t.source AND c.generation = t.generation AND c.turn_no = t.turn_no
  GROUP BY ALL
)
