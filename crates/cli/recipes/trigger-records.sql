-- Trigger records by kind, including those that did not open a turn.
SELECT kind, count(*) AS records
FROM triggers_v
GROUP BY kind
ORDER BY records DESC, kind
