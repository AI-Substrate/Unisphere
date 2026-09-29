#!/usr/bin/env python3
"""Fleet-scale prep measurement (AC-000c). Numbers only; no content.

Builds the release binary, then times against the real local corpus:
  cold      fresh target, every harness root prep covers
  unchanged immediate re-run
  append    re-run after --settle seconds of natural live growth
Each run records wall seconds, peak RSS (from /usr/bin/time -l), bytes read,
rows written, per-status source counts, pending tail bytes and state.json size.
Outputs go only to --scratch (gitignored); prints one JSON summary.

  python3 docs/plans/028-prep-canonical-tables/assets/proof/measure.py --scratch .harness/temp/prep-real
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time

POC_BASELINE = {"claude_cold_s": 3.46, "claude_unchanged_s": [0.05, 0.62], "claude_peak_rss_mb": 507}


def run(binary, target, extra):
    argv = ["/usr/bin/time", "-l", binary, "prep", "--target", target, "--json", *extra]
    started = time.monotonic()
    proc = subprocess.run(argv, capture_output=True, text=True)
    wall = time.monotonic() - started
    rss = re.search(r"(\d+)\s+maximum resident set size", proc.stderr)
    try:
        report = json.loads(proc.stdout.splitlines()[-1])["data"]
    except (IndexError, KeyError, json.JSONDecodeError):
        return {"exit": proc.returncode, "wall_s": round(wall, 3), "error": "no machine report"}
    by_status = {}
    skipped = {"symlinks": 0, "hidden": 0, "unreadable_entries": 0}
    for s in report["sets"]:
        for k, v in s["by_status"].items():
            by_status[k] = by_status.get(k, 0) + v
        for k in skipped:
            skipped[k] += s["skipped"][k]
    state = os.path.join(target, "state.json")
    return {
        "exit": proc.returncode,
        "wall_s": round(wall, 3),
        "peak_rss_mb": round(int(rss.group(1)) / 1_048_576, 1) if rss else None,
        "bytes_read": report["bytes_read"],
        "rows_written": report["rows_written"],
        "sources_by_status": by_status,
        "discovery_skipped": skipped,
        "pending_tail_bytes": report["pending_tail_bytes"],
        "sets": [(s["harness"], s["label"], s["supported"], s["discovered"]) for s in report["sets"]],
        "state_json_bytes": os.path.getsize(state) if os.path.exists(state) else 0,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", required=True)
    ap.add_argument("--settle", type=int, default=60, help="seconds of live growth before the append run")
    ap.add_argument("--threads", type=int, default=None)
    args, extra = ap.parse_known_args()
    repo = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    sha = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True, cwd=repo).stdout.strip()
    subprocess.run(["cargo", "build", "--release", "--locked", "-q", "-p", "unisphere-app"], cwd=repo, check=True)
    binary = os.path.join(repo, "target", "release", "unisphere")
    scratch = os.path.abspath(args.scratch)
    target = os.path.join(scratch, "measure-target")
    shutil.rmtree(target, ignore_errors=True)
    os.makedirs(scratch, exist_ok=True)
    if args.threads:
        extra = [*extra, "--threads", str(args.threads)]
    # Wall time and RSS depend on concurrent machine load; record it with every run.
    result = {"subject_sha": sha, "poc_baseline": POC_BASELINE, "extra_args": extra,
              "load_average_before": [round(v, 1) for v in os.getloadavg()]}
    result["cold"] = run(binary, target, extra)
    result["unchanged"] = run(binary, target, extra)
    time.sleep(args.settle)
    result["append"] = run(binary, target, extra)
    result["load_average_after"] = [round(v, 1) for v in os.getloadavg()]
    out = os.path.join(scratch, f"measure-{int(time.time())}.json")
    with open(out, "w") as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result))
    failed = any(result[k].get("exit") not in (0,) for k in ("cold", "unchanged", "append"))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
