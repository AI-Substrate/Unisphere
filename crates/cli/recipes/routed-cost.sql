-- Native cost and cache by the model that ran versus the model the session selected (a router's synthetic model).
SELECT s.harness, coalesce(c.selected_model, c.model) AS selected_model, c.model AS actual_model,
  c.selected_model IS NOT NULL AS routed, c.is_sidechain, count(*) AS calls,
  sum(c.input) AS input, sum(c.cache_read) AS cache_read,
  sum(c.cw_1h) AS cw_1h, sum(c.cw_5m) AS cw_5m, sum(c.cw_other) AS cw_other,
  string_agg(DISTINCT CAST(c.cache_write_ttl_s AS VARCHAR), ',') AS cache_write_ttl_s,
  sum(c.output) AS output, sum(c.native_cost) AS native_cost, c.native_cost_unit
FROM calls_v c
JOIN sources_v s ON s.source = c.source
GROUP BY ALL
ORDER BY native_cost DESC NULLS LAST, calls DESC, selected_model, actual_model
