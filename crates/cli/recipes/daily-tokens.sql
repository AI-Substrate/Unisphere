-- Daily token classes by harness, repository (project) and model. No prices are applied.
WITH {calls_x}
SELECT strftime(to_timestamp(ts_ms / 1000.0), '%Y-%m-%d') AS day, harness, project, model, count(*) AS calls,
  sum(input) AS input, sum(cache_read) AS cache_read, sum(cw_1h) AS cw_1h, sum(cw_5m) AS cw_5m, sum(output) AS output, sum(total_tokens) AS total_tokens
FROM calls_x
WHERE ts_ms IS NOT NULL
GROUP BY ALL
ORDER BY day, total_tokens DESC, harness, project, model
