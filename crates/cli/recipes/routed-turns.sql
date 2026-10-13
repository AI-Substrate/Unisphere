-- Which models ran for each routed turn: the router's decision, every phase in order and the model it committed.
SELECT r.source, r.turn_no, r.ts, r.selected_model, r.pattern, r.rule_id, r.route_source,
  r.planned_phases, r.primary_model, r.judge_model, r.repair_model, r.final_model, r.outcome,
  string_agg(p.kind || ':' || p.model || coalesce(' (' || p.verdict || ')', ''), ' -> '
    ORDER BY p.native_offset NULLS LAST, p.ts_ms NULLS LAST) AS phases_ran,
  r.phase_count, r.request_count, r.input, r.cache_read, r.cache_write, r.output,
  r.native_cost, r.native_cost_unit, r.latency_ms, r.duration_ms
FROM routes_v r
LEFT JOIN route_phases_v p ON p.source = r.source AND p.generation = r.generation AND p.route_id = r.route_id
GROUP BY ALL
ORDER BY any_value(r.ts_ms) NULLS LAST, r.source, r.turn_no
