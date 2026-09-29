-- Tokens, sessions, cold calls, compactions and turn openers per local day.
WITH {calls_x},
compactions AS (
  SELECT strftime(to_timestamp(ts_ms / 1000.0), '%Y-%m-%d') AS day, count(*) AS compactions
  FROM events_v WHERE kind = 'compaction' AND ts_ms IS NOT NULL GROUP BY 1
),
openers AS (
  SELECT strftime(to_timestamp(started_ts_ms / 1000.0), '%Y-%m-%d') AS day,
    count(*) FILTER (WHERE origin = 'peer') AS peer_turns,
    count(*) FILTER (WHERE origin = 'human') AS human_turns
  FROM turns_v WHERE started_ts_ms IS NOT NULL GROUP BY 1
),
days AS (
  SELECT strftime(to_timestamp(ts_ms / 1000.0), '%Y-%m-%d') AS day, count(DISTINCT source) AS sessions, count(*) AS calls,
    sum(input) AS input, sum(cache_read) AS cache_read, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m, sum(output) AS output,
    count(*) FILTER (WHERE cold) AS cold_calls
  FROM calls_x WHERE ts_ms IS NOT NULL GROUP BY 1
)
SELECT d.*, coalesce(c.compactions, 0) AS compactions,
  coalesce(o.peer_turns, 0) AS peer_turns, coalesce(o.human_turns, 0) AS human_turns
FROM days d LEFT JOIN compactions c USING (day) LEFT JOIN openers o USING (day)
ORDER BY d.day
