-- Turn-opening calls that arrived after the cache TTL, largest cache rewrite first.
WITH {calls_x}
SELECT source, session_id, seat, ts, origin AS trigger, sender, gap_s,
  ttl_s, context, cache_write AS cw, cold, pred_cold
FROM calls_x
WHERE call_in_turn = 1 AND pred_cold
ORDER BY cw DESC, ts_ms, source
