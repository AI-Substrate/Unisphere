-- What opened each turn and what the turn's calls cost in token classes.
WITH {turns_x}
SELECT source, session_id, seat, is_sub, turn_no, started_ts, origin AS trigger, sender, calls,
  input, cw_1h, cw_5m, cache_read, output,
  first_call_gap_s, first_call_context, max_context,
  cold, cold_write_tokens
FROM turns_x
ORDER BY started_ts_ms NULLS LAST, source, turn_no
