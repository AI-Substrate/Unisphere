#!/usr/bin/env python3
"""Live-file proof (AC-0002). Numbers only; no content leaves --scratch.

Part A (copy writer): the largest local Claude transcript is re-written into a
scratch root by a concurrent writer in random, non-LF-aligned chunks while prep
runs repeatedly against it. Every run must exit 0; pending tails must be
reported; the final incremental canonical totals must equal a fresh prep of the
completed copy.
Part B (real fleet): repeated prep over the real default roots while sessions
are live; every run must complete (exit 0, or 3 when a source is unreadable)
and live writing must never make a source unreadable: every run reports the
same unreadable count as the first (statically unreadable sources, such as
empty VS Code documents, are counted, not hidden).

  python3 docs/plans/028-prep-canonical-tables/assets/proof/live.py --scratch .harness/temp/prep-real
"""
import argparse
import glob
import json
import os
import random
import shutil
import subprocess
import sys
import threading
import time

import duckdb

CANONICAL = """
WITH cur AS (SELECT source, generation FROM read_parquet('{t}/tables/sources.parquet')),
calls AS (
  SELECT c.source, c.generation, c.msg_id, c.request_id,
         max(c.input) i, max(c.cw_1h) a, max(c.cw_5m) b, max(c.cache_read) r, max(c.output) o
  FROM read_parquet('{t}/tables/calls/*.parquet', union_by_name = true) c
  JOIN cur USING (source, generation) GROUP BY ALL),
turns AS (SELECT t.* FROM read_parquet('{t}/tables/turns/*.parquet', union_by_name = true) t JOIN cur USING (source, generation))
SELECT (SELECT count(*) FROM calls), (SELECT sum(i) FROM calls), (SELECT sum(a) FROM calls),
       (SELECT sum(b) FROM calls), (SELECT sum(r) FROM calls), (SELECT sum(o) FROM calls),
       (SELECT count(*) FROM turns)
"""


def prep(binary, target, extra):
    proc = subprocess.run([binary, "prep", "--target", target, "--json", *extra], capture_output=True, text=True)
    try:
        report = json.loads(proc.stdout.splitlines()[-1])["data"]
    except (IndexError, KeyError, json.JSONDecodeError):
        report = None
    return proc.returncode, report


def unreadable(report):
    return sum(s["by_status"].get("unreadable", 0) for s in report["sets"]) if report else -1


def totals(target):
    return list(duckdb.connect().execute(CANONICAL.format(t=target)).fetchone())


def part_a(binary, scratch, seed):
    sizes = []
    for path in glob.glob(os.path.expanduser("~/.claude/projects/*/*.jsonl")):
        try:
            info = os.lstat(path)  # symlinks and vanished files are not candidates
        except FileNotFoundError:
            continue
        if not os.path.islink(path):
            sizes.append((info.st_size, path))
    data = open(max(sizes)[1], "rb").read()
    root = os.path.join(scratch, "live-root")
    shutil.rmtree(root, ignore_errors=True)
    os.makedirs(os.path.join(root, "p"))
    live = os.path.join(root, "p", "live.jsonl")
    open(live, "wb").close()
    target = os.path.join(scratch, "live-target")
    fresh = os.path.join(scratch, "live-fresh")
    for t in (target, fresh):
        shutil.rmtree(t, ignore_errors=True)
    rng = random.Random(seed)
    done = threading.Event()

    def writer():
        pos = 0
        with open(live, "ab", buffering=0) as f:
            while pos < len(data):
                n = rng.randint(1, 262_144)
                f.write(data[pos:pos + n])
                pos += n
                time.sleep(rng.random() * 0.02)
        done.set()

    thread = threading.Thread(target=writer)
    thread.start()
    runs = failures = pending_seen = 0
    args = ["--root", f"claude-code:live={root}", "--no-default-roots"]
    while not done.is_set():
        code, report = prep(binary, target, args)
        runs += 1
        if code != 0 or unreadable(report) != 0:
            failures += 1
        elif report["pending_tail_bytes"] > 0:
            pending_seen += 1
    thread.join()
    code, _ = prep(binary, target, args)
    failures += code != 0
    code, _ = prep(binary, fresh, args)
    failures += code != 0
    incremental, complete = totals(target), totals(fresh)
    return {
        "source_bytes": len(data),
        "runs_during_writes": runs,
        "failed_runs": failures,
        "runs_reporting_pending_tail": pending_seen,
        "incremental_totals": incremental,
        "fresh_totals": complete,
        "equal": incremental == complete,
    }


def part_b(binary, scratch, runs, interval):
    target = os.path.join(scratch, "live-fleet-target")
    shutil.rmtree(target, ignore_errors=True)
    rows = []
    for i in range(runs):
        started = time.monotonic()
        code, report = prep(binary, target, [])
        by_status = {}
        for s in (report or {}).get("sets", []):
            for k, v in s["by_status"].items():
                by_status[k] = by_status.get(k, 0) + v
        rows.append({
            "run": i,
            "exit": code,
            "wall_s": round(time.monotonic() - started, 3),
            "unreadable": unreadable(report),
            "appended": by_status.get("appended", 0),
            "pending_tail_bytes": report["pending_tail_bytes"] if report else None,
        })
        time.sleep(interval)
    static = rows[0]["unreadable"] if rows else 0
    failed = sum(r["exit"] not in (0, 3) or r["unreadable"] != static for r in rows)
    return {"runs": rows, "static_unreadable": static, "failed_runs": failed}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", required=True)
    ap.add_argument("--seed", type=int, default=28)
    ap.add_argument("--fleet-runs", type=int, default=10)
    ap.add_argument("--fleet-interval", type=int, default=15)
    args = ap.parse_args()
    repo = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    sha = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True, cwd=repo).stdout.strip()
    subprocess.run(["cargo", "build", "--release", "--locked", "-q", "-p", "unisphere-app"], cwd=repo, check=True)
    binary = os.path.join(repo, "target", "release", "unisphere")
    scratch = os.path.abspath(args.scratch)
    os.makedirs(scratch, exist_ok=True)
    result = {"subject_sha": sha, "copy_writer": part_a(binary, scratch, args.seed),
              "fleet": part_b(binary, scratch, args.fleet_runs, args.fleet_interval)}
    with open(os.path.join(scratch, f"live-{int(time.time())}.json"), "w") as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result))
    ok = result["copy_writer"]["failed_runs"] == 0 and result["copy_writer"]["equal"] and result["fleet"]["failed_runs"] == 0
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
