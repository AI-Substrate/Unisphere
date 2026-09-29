-- Idle tax per seat: cache rewrites paid by turns that woke it after its cache TTL.
WITH {calls_x}
SELECT seat, harness, count(*) AS idle_wakes, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m,
  sum(cache_write) AS rewrite_tokens, max(context) AS max_context,
  count(*) FILTER (WHERE origin = 'peer') AS peer_opened
FROM calls_x
WHERE cold AND pred_cold
GROUP BY seat, harness
ORDER BY rewrite_tokens DESC, seat
