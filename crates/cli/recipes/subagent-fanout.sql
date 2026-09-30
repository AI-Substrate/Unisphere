-- Subagent fan-out: sidechains per parent session and their token classes against the parent's.
WITH {calls_x},
rooted AS (
  SELECT x.*, coalesce(ss.parent_session_id, ss.session_id, x.source) AS root_session
  FROM calls_x x LEFT JOIN sessions_v ss ON ss.source = x.source
)
SELECT root_session, any_value(harness) AS harness, any_value(project) AS project,
  count(DISTINCT source) FILTER (WHERE is_sub) AS sidechains,
  count(*) FILTER (WHERE NOT is_sub) AS parent_calls,
  count(*) FILTER (WHERE is_sub) AS sub_calls,
  sum(cache_read) FILTER (WHERE NOT is_sub) AS parent_cache_read,
  sum(cache_write) FILTER (WHERE NOT is_sub) AS parent_cache_write,
  sum(output) FILTER (WHERE NOT is_sub) AS parent_output,
  sum(cache_read) FILTER (WHERE is_sub) AS sub_cache_read,
  sum(cw_1h) FILTER (WHERE is_sub) AS sub_cw_1h,
  sum(cw_5m) FILTER (WHERE is_sub) AS sub_cw_5m,
  sum(output) FILTER (WHERE is_sub) AS sub_output,
  sum(total_tokens) FILTER (WHERE is_sub) AS sub_total_tokens
FROM rooted
GROUP BY root_session
HAVING count(DISTINCT source) FILTER (WHERE is_sub) > 0
ORDER BY sub_total_tokens DESC NULLS LAST, root_session
