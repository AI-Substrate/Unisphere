-- The message graph: peer messages and the turns they opened, per (from_seat, to_seat).
WITH {turns_x},
seats AS (SELECT source, coalesce(seat_hint, session_id, source) AS seat FROM sessions_v),
messages AS (
  SELECT t.sender AS from_seat, s.seat AS to_seat, count(*) AS n_messages,
    min(t.ts_ms) AS first_ms, max(t.ts_ms) AS last_ms
  FROM triggers_v t JOIN seats s USING (source)
  WHERE t.kind = 'peer'
  GROUP BY ALL
),
opened AS (
  SELECT sender AS from_seat, seat AS to_seat, count(*) AS n_turns_opened,
    count(*) FILTER (WHERE idle_wake) AS cold_wakes, sum(calls) AS calls,
    sum(input) AS input, sum(cache_read) AS cache_read, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m,
    sum(output) AS output
  FROM turns_x WHERE origin = 'peer'
  GROUP BY ALL
)
SELECT m.from_seat, m.to_seat, m.n_messages, coalesce(o.n_turns_opened, 0) AS n_turns_opened,
  coalesce(o.cold_wakes, 0) AS cold_wakes, o.calls, o.input, o.cache_read, o.cw_1h, o.cw_5m, o.output,
  strftime(to_timestamp(m.first_ms / 1000.0), '%Y-%m-%d %H:%M:%S') AS first_ts,
  strftime(to_timestamp(m.last_ms / 1000.0), '%Y-%m-%d %H:%M:%S') AS last_ts
FROM messages m
LEFT JOIN opened o ON o.from_seat IS NOT DISTINCT FROM m.from_seat AND o.to_seat = m.to_seat
ORDER BY m.n_messages DESC, m.from_seat, m.to_seat
