-- Tokens, cold rebuilds and peer messages per local hour.
WITH {calls_x},
messages AS (
  SELECT strftime(to_timestamp(ts_ms / 1000.0), '%Y-%m-%d %H:00') AS hour, count(*) AS pij_messages
  FROM triggers_v WHERE kind = 'peer' AND ts_ms IS NOT NULL GROUP BY 1
),
hours AS (
  SELECT strftime(to_timestamp(ts_ms / 1000.0), '%Y-%m-%d %H:00') AS hour, count(*) AS calls, count(DISTINCT source) AS sessions,
    sum(input) AS input, sum(cache_read) AS cache_read, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m, sum(output) AS output,
    count(*) FILTER (WHERE cold) AS cold_calls,
    coalesce(sum(cache_write) FILTER (WHERE cold), 0) AS cold_write_tokens
  FROM calls_x WHERE ts_ms IS NOT NULL GROUP BY 1
)
SELECT h.*, coalesce(m.pij_messages, 0) AS pij_messages
FROM hours h LEFT JOIN messages m USING (hour)
ORDER BY h.hour
