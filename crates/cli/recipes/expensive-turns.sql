-- The most expensive turns by total tokens, with the native address of their opener.
-- Fetch an opener with `unisphere prep record --target DIR --source SOURCE --offset OPENER_OFFSET --include-content`.
WITH {turns_x}
SELECT harness, project, seat, source, turn_no, started_ts, origin AS trigger, sender,
  opener_offset, first_call_offset, calls, input, cw_1h, cw_5m, cache_read, output,
  total_tokens, max_context
FROM turns_x
WHERE calls > 0
ORDER BY total_tokens DESC, source, turn_no
LIMIT 50
