#!/usr/bin/env python3
"""Real-corpus Claude parity (AC-0004, vd-000a). Numbers only; no content, no identifiers.

1. Copies the consumer's reference parser (extract.py) into --scratch and runs
   it there; its CSVs contain excerpts and never leave the gitignored scratch.
2. Runs `unisphere prep` (release build) over the default Claude root into
   --scratch/parity-target.
3. Loads TARGET/views.sql into DuckDB and compares the canonical views with the
   reference output: file-F whole-file totals, the four window totals of the
   use-case brief, every reference call row (counters, model, call_in_turn,
   trigger, sender, gap), compactions, triggers and hidden rows. Every
   divergence is counted and classified; nothing is rounded away.

  python3 docs/plans/028-prep-canonical-tables/assets/proof/parity.py --scratch .harness/temp/prep-real
"""
import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
import time

import duckdb

FEEDBACK = os.path.expanduser("~/games/unasphere/scratch/unisphere-feedback")
AEST = dt.timezone(dt.timedelta(hours=10))
PREFIX = "claude-code/default/"


def ms(*args):
    return int(dt.datetime(*args, tzinfo=AEST).timestamp() * 1000)


XLO, XHI = ms(2026, 9, 23, 12), ms(2026, 9, 28, 12)
LO, HI = ms(2026, 9, 24), ms(2026, 9, 28)
# Use-case brief section 4 (aggregate numbers only): calls, input, cw_1h, cw_5m, cache_read, output
# and files for the window rows.
BRIEF = {
    "file_f": [336, 4078, 4073840, 0, 105199616, 316933],
    "extract_window": [129, 55607, 125006, 161327570, 10328326, 18515531680, 41261643],
    "analysis_window": [119, 53691, 120904, 148719523, 9805657, 17875839027, 39717869],
    "analysis_main": [63, 51608, 116712, 148708908, 0, 17610578342, 38066136],
    "analysis_sub": [56, 2083, 4192, 10615, 9805657, 265260685, 1651733],
}


def file_f_key():
    """Relative path of the brief's file F, read at runtime so no identifier is committed."""
    text = open(os.path.join(FEEDBACK, "usecase-brief.md")).read()
    match = re.search(r"File F, whole file.*?`([^`]+\.jsonl)`", text, re.S)
    path = match.group(1)
    return os.path.relpath(path, os.path.expanduser("~/.claude/projects"))


def run_reference(scratch):
    ref = os.path.join(scratch, "ref")
    shutil.rmtree(ref, ignore_errors=True)
    os.makedirs(ref)
    shutil.copy(os.path.join(FEEDBACK, "extract.py"), ref)
    started = time.monotonic()
    subprocess.run(["nice", "-n", "10", sys.executable, os.path.join(ref, "extract.py")], check=True,
                   stdout=subprocess.DEVNULL)
    return ref, round(time.monotonic() - started, 2)


def run_prep(repo, scratch):
    subprocess.run(["cargo", "build", "--release", "--locked", "-q", "-p", "unisphere-app"], cwd=repo, check=True)
    target = os.path.join(scratch, "parity-target")
    shutil.rmtree(target, ignore_errors=True)
    started = time.monotonic()
    proc = subprocess.run([os.path.join(repo, "target", "release", "unisphere"), "prep", "--target", target,
                           "--harness", "claude-code", "--json"], capture_output=True, text=True)
    report = json.loads(proc.stdout.splitlines()[-1])["data"]
    return target, {"exit": proc.returncode, "wall_s": round(time.monotonic() - started, 2),
                    "by_status": report["sets"][0]["by_status"], "skipped": report["sets"][0]["skipped"],
                    "rows_written": report["rows_written"]}


def connect(target, ref):
    con = duckdb.connect()
    con.execute(f"SET file_search_path = '{target}'")
    con.execute(open(os.path.join(target, "views.sql")).read())
    con.execute(f"CREATE TABLE ref_calls AS SELECT * FROM read_csv('{ref}/calls.csv', header=true)")
    con.execute(f"CREATE TABLE ref_comp AS SELECT * FROM read_csv('{ref}/compactions.csv', header=true)")
    con.execute(f"CREATE TABLE ref_trig AS SELECT file, ts_utc, kind, sender, pij_msg_id, chars FROM read_csv('{ref}/triggers.csv', header=true, all_varchar=true)")
    con.execute(f"CREATE TABLE ref_hidden AS SELECT file, ts_utc, kind, last_context, gap_s FROM read_csv('{ref}/hidden.csv', header=true, all_varchar=true)")
    n = len(PREFIX)
    con.execute(f"""CREATE TABLE mine AS
      SELECT substr(c.source, {n + 1}) AS file, c.ts, c.ts_ms, c.model,
             coalesce(c.input, 0) AS input, coalesce(c.cw_1h, 0) AS cw_1h, coalesce(c.cw_5m, 0) AS cw_5m,
             coalesce(c.cache_read, 0) AS cache_read, coalesce(c.output, 0) AS output,
             c.gap_ms, c.turn_no, c.call_in_turn, t.origin AS trigger, coalesce(t.sender, '') AS sender,
             s.is_sub
      FROM calls_v c
      JOIN sources_v s USING (source, generation)
      LEFT JOIN turns_v t ON t.source = c.source AND t.generation = c.generation AND t.turn_no = c.turn_no
      WHERE c.source LIKE '{PREFIX}%'""")
    con.execute(f"CREATE TABLE mine_events AS SELECT substr(source, {n + 1}) AS file, * EXCLUDE (source) FROM events_v WHERE source LIKE '{PREFIX}%'")
    con.execute(f"CREATE TABLE mine_trig AS SELECT substr(source, {n + 1}) AS file, * EXCLUDE (source) FROM triggers_v WHERE source LIKE '{PREFIX}%'")
    return con


def one(con, sql):
    row = con.execute(sql).fetchone()
    return [int(v) if isinstance(v, (int, float)) and v is not None else v for v in row]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", required=True)
    ap.add_argument("--reuse-ref", action="store_true", help="reuse an existing scratch reference run")
    args = ap.parse_args()
    repo = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    sha = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True, cwd=repo).stdout.strip()
    scratch = os.path.abspath(args.scratch)
    os.makedirs(scratch, exist_ok=True)
    ref = os.path.join(scratch, "ref")
    ref_wall = None
    if not (args.reuse_ref and os.path.exists(os.path.join(ref, "calls.csv"))):
        ref, ref_wall = run_reference(scratch)
    target, prep = run_prep(repo, scratch)
    con = connect(target, ref)
    f = file_f_key().replace("'", "''")
    X = f"ts_ms >= {XLO} AND ts_ms < {XHI}"
    A = f"ts_ms >= {LO} AND ts_ms < {HI}"
    ref_ms = "epoch_ms(CAST(ts_utc AS TIMESTAMPTZ))"
    result = {"subject_sha": sha, "reference_wall_s": ref_wall, "prep": prep}

    # Aggregate totals: prep vs a fresh reference run vs the brief's published numbers.
    scopes = {
        "file_f": (f"file = '{f}'", False),
        "extract_window": (X, True),
        "analysis_window": (A, True),
        "analysis_main": (f"{A} AND NOT is_sub", True),
        "analysis_sub": (f"{A} AND is_sub", True),
    }
    ref_where = {
        "file_f": f"file = '{f}'",
        "extract_window": "true",
        "analysis_window": f"{ref_ms} >= {LO} AND {ref_ms} < {HI}",
        "analysis_main": f"{ref_ms} >= {LO} AND {ref_ms} < {HI} AND is_sub = 0",
        "analysis_sub": f"{ref_ms} >= {LO} AND {ref_ms} < {HI} AND is_sub = 1",
    }
    agg = {}
    for name, (where, files) in scopes.items():
        cols = "count(DISTINCT file), " if files else ""
        agg[name] = {
            "prep": one(con, f"SELECT {cols}count(*), sum(input), sum(cw_1h), sum(cw_5m), sum(cache_read), sum(output) FROM mine WHERE {where}"),
            "reference_now": one(con, f"SELECT {cols}count(*), sum(input), sum(cw_1h), sum(cw_5m), sum(cache_read), sum(output) FROM ref_calls WHERE {ref_where[name]}"),
            "brief": BRIEF[name],
        }
        # The reference writes extract-window rows only, so file F's fresh reference
        # is window-limited; its whole-file oracle is the brief.
        agg[name]["prep_equals_reference_now"] = (
            None if name == "file_f" else agg[name]["prep"] == agg[name]["reference_now"])
        agg[name]["prep_equals_brief"] = agg[name]["prep"] == agg[name]["brief"]
    result["totals"] = agg

    # Row-level parity over the extract window.
    con.execute(f"CREATE TABLE mine_x AS SELECT *, round(gap_ms / 1000.0, 1) AS gap_s FROM mine WHERE {X}")
    j = "FROM ref_calls r JOIN mine_x m ON m.file = r.file AND m.ts = r.ts_utc"
    result["call_rows"] = dict(zip(
        ["reference", "prep", "joined_on_file_ts"],
        one(con, f"SELECT (SELECT count(*) FROM ref_calls), (SELECT count(*) FROM mine_x), (SELECT count(*) {j})")))
    result["call_field_mismatches"] = dict(zip(
        ["usage", "model", "call_in_turn", "trigger", "sender", "gap_window_first_only_in_reference",
         "gap_beyond_rounding", "gap_rounding_ties"],
        one(con, f"""SELECT
          sum(CASE WHEN r.input <> m.input OR r.cw_1h <> m.cw_1h OR r.cw_5m <> m.cw_5m OR r.cache_read <> m.cache_read OR r.output <> m.output THEN 1 ELSE 0 END),
          sum(CASE WHEN coalesce(r.model, '') <> coalesce(m.model, '') THEN 1 ELSE 0 END),
          sum(CASE WHEN r.call_in_turn <> m.call_in_turn THEN 1 ELSE 0 END),
          sum(CASE WHEN r.trigger <> m.trigger THEN 1 ELSE 0 END),
          sum(CASE WHEN coalesce(r.sender, '') <> m.sender THEN 1 ELSE 0 END),
          sum(CASE WHEN r.gap_s = -1 AND m.gap_ms <> -1 THEN 1 ELSE 0 END),
          sum(CASE WHEN r.gap_s <> -1 AND abs(r.gap_s * 1000 - m.gap_ms) > 50 THEN 1 ELSE 0 END),
          sum(CASE WHEN r.gap_s <> -1 AND abs(r.gap_s * 1000 - m.gap_ms) <= 50 AND r.gap_s <> m.gap_s THEN 1 ELSE 0 END)
          {j}""")))
    result["trigger_mismatch_pairs"] = [list(r) for r in con.execute(
        f"SELECT r.trigger, m.trigger, count(*) {j} WHERE r.trigger <> m.trigger GROUP BY 1, 2 ORDER BY 3 DESC").fetchall()]
    result["compactions"] = dict(zip(["reference", "prep", "equal_rows"], one(con, f"""SELECT
        (SELECT count(*) FROM ref_comp),
        (SELECT count(*) FROM mine_events WHERE kind = 'compaction' AND {X}),
        (SELECT count(*) FROM ref_comp r JOIN mine_events e ON e.file = r.file AND e.ts = r.ts_utc AND e.kind = 'compaction'
          WHERE r.pre_tokens IS NOT DISTINCT FROM e.pre_tokens AND r.post_tokens IS NOT DISTINCT FROM e.post_tokens
            AND r.duration_ms IS NOT DISTINCT FROM e.duration_ms AND r.trigger IS NOT DISTINCT FROM e.trigger)""")))
    result["triggers_multiset"] = dict(zip(["reference", "prep", "reference_only", "prep_only"], one(con, f"""
        WITH r AS (SELECT file, ts_utc AS ts, kind, coalesce(sender, '') AS sender, coalesce(pij_msg_id, '') AS pid, CAST(chars AS BIGINT) AS chars FROM ref_trig),
             p AS (SELECT file, ts, kind, coalesce(sender, '') AS sender, coalesce(pij_msg_id, '') AS pid, chars FROM mine_trig WHERE {X})
        SELECT (SELECT count(*) FROM r), (SELECT count(*) FROM p),
               (SELECT count(*) FROM (SELECT * FROM r EXCEPT ALL SELECT * FROM p)),
               (SELECT count(*) FROM (SELECT * FROM p EXCEPT ALL SELECT * FROM r))""")))
    result["hidden_rows"] = [list(r) for r in con.execute(f"""
        SELECT r.kind, r.n, p.n FROM (SELECT kind, count(*) n FROM ref_hidden GROUP BY 1) r LEFT JOIN
          (SELECT CASE kind WHEN 'recap' THEN 'recap' ELSE 'synthetic' END AS kind, count(*) n FROM mine_events
           WHERE kind IN ('recap', 'limit_notice') AND {X} GROUP BY 1) p USING (kind) ORDER BY 1""").fetchall()]
    out = os.path.join(scratch, f"parity-{int(time.time())}.json")
    with open(out, "w") as fh:
        json.dump(result, fh, indent=1)
    print(json.dumps(result))
    mism = result["call_field_mismatches"]
    exact = (all(v["prep_equals_reference_now"] is not False for v in agg.values())
             and agg["file_f"]["prep_equals_brief"]
             and result["call_rows"]["reference"] == result["call_rows"]["joined_on_file_ts"]
             and not any(mism[k] for k in ("usage", "model", "call_in_turn", "trigger", "sender", "gap_beyond_rounding")))
    sys.exit(0 if exact else 1)


if __name__ == "__main__":
    main()
