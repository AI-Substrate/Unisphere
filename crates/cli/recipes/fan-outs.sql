-- One sender, the same message (body hash), many seats within 120 s.
WITH {turns_x},
peer AS (
  SELECT *, lag(opener_ts_ms) OVER (PARTITION BY sender, body_key ORDER BY opener_ts_ms) AS prev_ms
  FROM turns_x WHERE origin = 'peer' AND opener_ts_ms IS NOT NULL
),
batched AS (
  SELECT *, sum(CASE WHEN opener_ts_ms - prev_ms <= 120000 THEN 0 ELSE 1 END)
    OVER (PARTITION BY sender, body_key ORDER BY opener_ts_ms) AS batch
  FROM peer
)
SELECT sender, strftime(to_timestamp(min(opener_ts_ms) / 1000.0), '%Y-%m-%d %H:%M:%S') AS first_ts,
  count(DISTINCT seat) AS recipients, count(*) AS turns_opened,
  count(*) FILTER (WHERE idle_wake) AS cold_wakes, sum(calls) AS calls,
  sum(input) AS input, sum(cache_read) AS cache_read, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m,
  sum(output) AS output, body_key
FROM batched
GROUP BY sender, body_key, batch
ORDER BY recipients DESC, turns_opened DESC, first_ts, sender
