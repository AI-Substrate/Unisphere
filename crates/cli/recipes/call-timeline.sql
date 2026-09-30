-- Every canonical call in order, per seat, with its turn opener and cold flags.
WITH {calls_x}
SELECT source, harness, project, session_id, seat, agent_id, is_sub, ts, model,
  input, cw_1h, cw_5m, cache_read, output, context,
  gap_s, turn_no, call_in_turn, origin AS trigger, sender,
  cold, pred_cold, ttl_s
FROM calls_x
ORDER BY ts_ms NULLS LAST, source, turn_no, call_in_turn
