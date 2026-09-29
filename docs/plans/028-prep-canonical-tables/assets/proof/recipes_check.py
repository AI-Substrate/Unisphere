#!/usr/bin/env python3
"""Research recipes against external DuckDB (AC-0009, vd-000c). Numbers only.

For every recipe listed by `unisphere prep recipes --json`, runs the documented
one-line command `unisphere prep recipe NAME --target DIR | duckdb -csv` against
(1) a synthetic target prepped from the committed content-free Claude fixtures
and (2) a real target prepped from this machine's default roots. Records exit
status, stderr presence and result row counts only; recipe output never leaves
the process. Without a duckdb executable the check fails with the install hint.

  python3 docs/plans/028-prep-canonical-tables/assets/proof/recipes_check.py --scratch .harness/temp/prep-real
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time


def prep(binary, target, extra):
    shutil.rmtree(target, ignore_errors=True)
    proc = subprocess.run([binary, "prep", "--target", target, "--json", *extra], capture_output=True, text=True)
    return proc.returncode


def run_recipe(binary, duckdb, name, target, cwd):
    render = subprocess.run([binary, "prep", "recipe", name, "--target", target], capture_output=True, cwd=cwd)
    if render.returncode != 0:
        return {"render_exit": render.returncode}
    query = subprocess.run([duckdb, "-csv"], input=render.stdout, capture_output=True, cwd=cwd)
    lines = [l for l in query.stdout.decode(errors="replace").splitlines() if l.strip()]
    return {
        "render_exit": 0,
        "duckdb_exit": query.returncode,
        "stderr": bool(query.stderr.strip()),
        "rows": max(len(lines) - 1, 0),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", required=True)
    args = ap.parse_args()
    duckdb = shutil.which("duckdb")
    if not duckdb:
        print(json.dumps({"error": "duckdb not found on PATH; install it (e.g. `brew install duckdb`) to run the recipes"}))
        sys.exit(1)
    repo = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    sha = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True, cwd=repo).stdout.strip()
    subprocess.run(["cargo", "build", "--release", "--locked", "-q", "-p", "unisphere-app"], cwd=repo, check=True)
    binary = os.path.join(repo, "target", "release", "unisphere")
    scratch = os.path.abspath(args.scratch)
    os.makedirs(scratch, exist_ok=True)
    version = subprocess.run([duckdb, "--version"], capture_output=True, text=True).stdout.split()[0]
    listed = json.loads(subprocess.run([binary, "prep", "recipes", "--json"], capture_output=True, text=True, check=True).stdout)
    names = [r["name"] for r in listed["data"]["recipes"]]
    targets = {
        "synthetic": (os.path.join(scratch, "recipes-synthetic"),
                      ["--no-default-roots", "--root", "claude-code:fixture=" + os.path.join(repo, "crates/adapter-claude/tests/fixtures/prep")]),
        "real": (os.path.join(scratch, "recipes-real"), []),
    }
    result = {"subject_sha": sha, "duckdb": version, "recipes": len(names), "targets": {}}
    ok = True
    for label, (target, extra) in targets.items():
        code = prep(binary, target, extra)
        runs = {name: run_recipe(binary, duckdb, name, target, scratch) for name in names}
        failed = [n for n, r in runs.items() if r.get("render_exit") or r.get("duckdb_exit") or r.get("stderr")]
        result["targets"][label] = {
            "prep_exit": code,
            "failed": failed,
            "empty": [n for n, r in runs.items() if r.get("rows") == 0],
            "rows": {n: r.get("rows") for n, r in runs.items()},
        }
        ok = ok and code == 0 and not failed
    with open(os.path.join(scratch, f"recipes-{int(time.time())}.json"), "w") as fh:
        json.dump(result, fh, indent=1)
    print(json.dumps(result))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
