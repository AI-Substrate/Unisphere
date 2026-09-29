#!/usr/bin/env python3
"""Real-corpus per-harness coverage (AC-0006, vd-0017). Numbers only.

Preps every catalogued default root on this machine into --scratch/coverage-target
(fresh), then reports per harness (catalogue descriptor id): sources by status,
discovery skips, rows per table, and the share of rows carrying model, usage,
timestamps and native ids — so explicit nulls are visible per dialect rather than
estimated. Nothing but counts and ratios is printed or stored.

  python3 docs/plans/028-prep-canonical-tables/assets/proof/coverage.py --scratch .harness/temp/prep-real
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time

import duckdb


def share(numerator, denominator):
    return round(numerator / denominator, 4) if denominator else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", required=True)
    args, extra = ap.parse_known_args()
    repo = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    sha = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True, cwd=repo).stdout.strip()
    subprocess.run(["cargo", "build", "--release", "--locked", "-q", "-p", "unisphere-app"], cwd=repo, check=True)
    binary = os.path.join(repo, "target", "release", "unisphere")
    scratch = os.path.abspath(args.scratch)
    target = os.path.join(scratch, "coverage-target")
    shutil.rmtree(target, ignore_errors=True)
    os.makedirs(scratch, exist_ok=True)
    started = time.monotonic()
    proc = subprocess.run([binary, "prep", "--target", target, "--json", *extra], capture_output=True, text=True)
    report = json.loads(proc.stdout.splitlines()[-1])["data"]
    result = {
        "subject_sha": sha,
        "exit": proc.returncode,
        "wall_s": round(time.monotonic() - started, 2),
        "load_average": [round(v, 1) for v in os.getloadavg()],
        "sets": {
            s["harness"]: {"supported": s["supported"], "discovered": s["discovered"],
                           "skipped": s["skipped"], "by_status": s["by_status"]}
            for s in report["sets"]
        },
        "harnesses": {},
    }
    con = duckdb.connect()
    con.execute(f"SET file_search_path = '{target}'")
    con.execute(open(os.path.join(target, "views.sql")).read())
    harnesses = [r[0] for r in con.execute("SELECT DISTINCT harness FROM sources_v ORDER BY 1").fetchall()]
    for h in harnesses:
        src = f"source IN (SELECT source FROM sources_v WHERE harness = '{h}')"
        calls = con.execute(f"""SELECT count(*), count(model), count(ts_ms), count(input), count(output),
            count(cache_read), count(cw_1h), count(msg_id), count(request_id), count(native_offset), count(native_key)
            FROM calls_v WHERE {src}""").fetchone()
        n = calls[0]
        tools = con.execute(f"SELECT count(*), count(name), count(outcome), count(duration_ms) FROM tool_uses_v WHERE {src}").fetchone()
        result["harnesses"][h] = {
            "sources": con.execute(f"SELECT count(*) FROM sources_v WHERE harness = '{h}'").fetchone()[0],
            "sessions_with_id": con.execute(f"SELECT count(session_id) FROM sessions_v WHERE {src}").fetchone()[0],
            "rows": {
                "calls": n,
                "turns": con.execute(f"SELECT count(*) FROM turns_v WHERE {src}").fetchone()[0],
                "triggers": con.execute(f"SELECT count(*) FROM triggers_v WHERE {src}").fetchone()[0],
                "events": con.execute(f"SELECT count(*) FROM events_v WHERE {src}").fetchone()[0],
                "compactions": con.execute(f"SELECT count(*) FROM compactions_v WHERE {src}").fetchone()[0],
                "tool_uses": tools[0],
            },
            "call_field_presence": {
                "model": share(calls[1], n), "ts": share(calls[2], n), "input": share(calls[3], n),
                "output": share(calls[4], n), "cache_read": share(calls[5], n), "cw_1h": share(calls[6], n),
                "msg_id": share(calls[7], n), "request_id": share(calls[8], n),
                "native_offset": share(calls[9], n), "native_key": share(calls[10], n),
            },
            "tool_field_presence": {
                "name": share(tools[1], tools[0]), "outcome": share(tools[2], tools[0]), "duration_ms": share(tools[3], tools[0]),
            },
        }
    with open(os.path.join(scratch, f"coverage-{int(time.time())}.json"), "w") as fh:
        json.dump(result, fh, indent=1)
    print(json.dumps(result))
    unreadable = sum(s["by_status"].get("unreadable", 0) for s in result["sets"].values())
    unsupported = sum(1 for s in result["sets"].values() if not s["supported"])
    sys.exit(0 if proc.returncode == 0 and unreadable == 0 and unsupported == 0 else 1)


if __name__ == "__main__":
    main()
