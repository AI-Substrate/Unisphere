-- Who woke whom: turns opened by each peer sender and what they cost.
WITH {turns_x}
SELECT sender, count(*) AS turns_opened, count(DISTINCT session_id) AS recipient_sessions,
  count(*) FILTER (WHERE calls <= 3) AS turns_le3_calls, sum(calls) AS calls,
  sum(cache_read) AS cache_read, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m, sum(output) AS output,
  count(*) FILTER (WHERE idle_wake) AS cold_wakes, sum(cold_write_tokens) AS cold_write_tokens,
  sum(total_tokens) AS total_tokens
FROM turns_x
WHERE origin = 'peer'
GROUP BY sender
ORDER BY total_tokens DESC, sender
