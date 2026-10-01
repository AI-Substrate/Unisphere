-- Cumulative token classes in 10-minute buckets from the week start.
-- week_start_ms defaults to seven days before the latest call. Edit it to replay a quota week.
WITH {calls_x},
params AS (SELECT max(ts_ms) - 7 * 86400000 AS week_start_ms FROM calls_v),
buckets AS (
  SELECT (floor(ts_ms / 600000) + 1) * 600000 AS bucket_end_ms,
    sum(input) AS input, sum(cache_read) AS cache_read, sum(cw_1h) AS cw_1h,
    sum(cw_5m) AS cw_5m, sum(output) AS output
  FROM calls_x, params
  WHERE ts_ms >= week_start_ms
  GROUP BY 1
)
SELECT strftime(to_timestamp(bucket_end_ms / 1000.0), '%Y-%m-%d %H:%M') AS bucket_end,
  sum(input) OVER w AS cum_input, sum(cache_read) OVER w AS cum_cache_read,
  sum(cw_1h) OVER w AS cum_cw_1h, sum(cw_5m) OVER w AS cum_cw_5m, sum(output) OVER w AS cum_output
FROM buckets
WINDOW w AS (ORDER BY bucket_end_ms ROWS UNBOUNDED PRECEDING)
ORDER BY bucket_end_ms
