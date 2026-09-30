-- Does cost scale with context? Calls and token classes per 100k context band.
WITH {calls_x}
SELECT CASE WHEN context >= 800000 THEN '800k+'
    WHEN context >= 600000 THEN '600-800k'
    ELSE CAST(CAST(floor(context / 100000) * 100 AS BIGINT) AS VARCHAR) || '-'
      || CAST(CAST(floor(context / 100000) * 100 + 100 AS BIGINT) AS VARCHAR) || 'k'
  END AS band,
  count(*) AS calls, count(DISTINCT source) AS sessions, sum(cache_read) AS cache_read,
  sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m, sum(output) AS output,
  count(*) FILTER (WHERE cold) AS cold_calls,
  sum(greatest(coalesce(cache_read, 0) - 200000, 0)) AS cache_read_above_200k
FROM calls_x
WHERE context IS NOT NULL
GROUP BY band
ORDER BY min(context)
