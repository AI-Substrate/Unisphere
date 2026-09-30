-- Spend by what opened the turn (turn origin), ordered by total tokens.
WITH {turns_x}
SELECT origin AS trigger, count(*) AS turns, sum(calls) AS calls,
  median(calls) AS median_calls_per_turn,
  sum(input) AS input, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m, sum(cache_read) AS cache_read,
  sum(output) AS output, count(*) FILTER (WHERE idle_wake) AS cold_wakes,
  sum(cold_write_tokens) AS cold_write_tokens, sum(total_tokens) AS total_tokens
FROM turns_x
GROUP BY origin
ORDER BY total_tokens DESC, trigger
