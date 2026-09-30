-- Peer amplification: turns one seat's messages opened in another, and their tokens.
WITH {turns_x}
SELECT sender AS from_seat, seat AS to_seat, count(*) AS turns_opened,
  count(*) FILTER (WHERE calls <= 3) AS turns_le3_calls, sum(calls) AS calls,
  sum(input) AS input, sum(cache_read) AS cache_read, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m,
  sum(output) AS output, count(*) FILTER (WHERE idle_wake) AS cold_wakes,
  sum(total_tokens) AS total_tokens
FROM turns_x
WHERE origin = 'peer'
GROUP BY ALL
ORDER BY total_tokens DESC, from_seat, to_seat
