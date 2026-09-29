-- Structure counts to reconcile against a reference parser (brief section 4 D).
WITH {calls_x}
SELECT
  (SELECT count(*) FROM calls_x) AS calls,
  (SELECT sum(records) FROM calls_x) AS usage_records,
  (SELECT sum(records) FROM calls_x) - (SELECT count(*) FROM calls_x) AS duplicates_merged,
  (SELECT count(*) FROM turns_v) AS turns,
  (SELECT count(*) FROM turns_v WHERE origin = 'peer') AS peer_turns,
  (SELECT count(*) FROM turns_v WHERE origin = 'human') AS human_turns,
  (SELECT count(*) FROM events_v WHERE kind = 'compaction') AS compactions,
  (SELECT count(*) FROM events_v WHERE kind = 'compaction' AND trigger = 'manual') AS compactions_manual,
  (SELECT count(*) FROM events_v WHERE kind = 'compaction' AND trigger = 'auto') AS compactions_auto,
  (SELECT median(pre_tokens) FROM events_v WHERE kind = 'compaction') AS median_pre_tokens,
  (SELECT count(*) FROM events_v WHERE kind = 'recap') AS recaps,
  (SELECT count(*) FROM events_v WHERE kind = 'limit_notice') AS limit_notices,
  (SELECT count(*) FROM calls_x WHERE cold) AS cold_calls,
  (SELECT count(*) FROM calls_x WHERE cold AND pred_cold) AS idle_wakes,
  (SELECT count(*) FROM calls_x WHERE cold AND pred_cold AND origin = 'peer') AS idle_wakes_peer,
  (SELECT count(*) FROM calls_x WHERE cold AND NOT pred_cold) AS rebuilds,
  (SELECT round(avg(context)) FROM calls_x) AS mean_context
