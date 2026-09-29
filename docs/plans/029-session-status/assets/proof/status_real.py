#!/usr/bin/env python3
"""Plan 029 real-machine proof (vd-0008). Numbers only.

Reads this machine's real Claude transcripts read-only through the built
`unisphere` binary and a scratch SDK bench. Every write goes to --scratch
(gitignored .harness/temp). Committed evidence carries counts and timings,
never content, paths or ids.

  1. largest transcript by --session and a pij seat by --pij: cold wall time,
     exit 0, no limit error
  2. concurrent appender on a scratch copy: every status run exits 0
  3. 50 most recent transcripts through StatusService::status_incremental:
     cold, then warm passes with caller-held cursors (target < 100 ms)
  4. native pane lookup (pij removed from PATH): median per pane (about 50 ms, bounded by one ps call)
  5. a real session that switched model reports the newest model
"""

import argparse, json, os, shutil, statistics, subprocess, sys, threading, time
from pathlib import Path

REPO = Path(__file__).resolve().parents[5]
HOME = Path.home()
PROJECTS = HOME / ".claude" / "projects"

BENCH = r'''
use std::{env, sync::Arc, time::Instant};
use unisphere_loader_query::status_target::{CommandRunner, PsProcessTable, StatusTargetResolver, SystemCommandRunner, SystemFs};
use unisphere_sdk::{prep::{PrepBinding, default_set}, status::{StatusQuery, StatusService, StatusTarget, TargetResolver}};
fn ms(t: Instant) -> f64 { t.elapsed().as_secs_f64() * 1000.0 }
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args[0].as_str() {
        "warm" => {
            let service = StatusService::new(
                vec![PrepBinding { fold: Arc::new(unisphere_adapter_claude::ClaudePrepFold), loader: Arc::new(unisphere_loader_jsonl::FileSessionLoader) }],
                vec![default_set("claude-code", args[1].clone().into())]);
            let now = 1_800_000_000_000;
            let targets: Vec<StatusTarget> = args[2..].iter().map(|p| StatusTarget {
                harness: "claude-code".into(),
                session_id: std::path::Path::new(p).file_stem().unwrap().to_string_lossy().into_owned(),
                transcript: Some(p.into()) }).collect();
            let t = Instant::now();
            let mut cursors: Vec<_> = targets.iter().map(|target| service.status_incremental(target, None, now).map(|(_, c)| c)).collect();
            let cold = ms(t);
            let failed = cursors.iter().filter(|c| c.is_err()).count();
            let mut warm = Vec::new();
            let mut bytes = 0u64;
            for _ in 0..5 {
                let t = Instant::now();
                for (target, cursor) in targets.iter().zip(cursors.iter_mut()) {
                    if let Ok(held) = cursor {
                        if let Ok((status, next)) = service.status_incremental(target, Some(held), now) {
                            bytes += status.source.bytes_read; *held = next;
                        }
                    }
                }
                warm.push(ms(t));
            }
            println!("{}", serde_json::json!({"n": targets.len(), "failed": failed, "cold_ms": cold, "warm_ms": warm, "warm_bytes_read": bytes}));
        }
        "panes" => {
            let runner: Arc<dyn CommandRunner> = Arc::new(SystemCommandRunner::default());
            let resolver = StatusTargetResolver::new(runner.clone(), Arc::new(PsProcessTable::new(runner)), Arc::new(SystemFs), env::var("HOME").unwrap().into());
            let mut out = Vec::new();
            for pane in &args[1..] {
                let mut times = Vec::new(); let mut ok = false;
                for _ in 0..10 { let t = Instant::now(); ok = resolver.resolve(&StatusQuery::Pane(pane.clone())).is_ok(); times.push(ms(t)); }
                times.sort_by(|a, b| a.partial_cmp(b).unwrap());
                out.push(serde_json::json!({"ok": ok, "median_ms": times[5], "max_ms": times[9]}));
            }
            println!("{}", serde_json::Value::from(out));
        }
        _ => panic!("mode"),
    }
}
'''


def run(argv, env=None, timeout=600):
    t = time.perf_counter()
    p = subprocess.run(argv, capture_output=True, text=True, env=env, timeout=timeout)
    return p, (time.perf_counter() - t) * 1000


def status(binary, args, env=None):
    p, wall = run([str(binary), "sessions", "status", *args, "--json"], env=env)
    try:
        body = json.loads(p.stdout)
    except ValueError:
        body = {}
    result = (body.get("data", {}).get("results") or [{}])[0]
    return p.returncode, wall, result


def transcripts():
    return [p for p in PROJECTS.glob("*/*.jsonl") if p.is_file()]


def build(scratch):
    subprocess.run(["cargo", "build", "--locked", "--release", "-q", "-p", "unisphere-app"], cwd=REPO, check=True)
    bench = scratch / "bench"
    (bench / "src").mkdir(parents=True, exist_ok=True)
    deps = "".join(f'unisphere-{n}={{path={json.dumps(str(REPO / "crates" / n))}}}\n'
                   for n in ["sdk", "loader-jsonl", "adapter-claude", "loader-query"])
    (bench / "Cargo.toml").write_text(
        f'[package]\nname="status-bench"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n{deps}serde_json="1"\n')
    (bench / "src/main.rs").write_text(BENCH)
    subprocess.run(["cargo", "build", "--release", "-q", "--manifest-path", str(bench / "Cargo.toml"),
                    "--target-dir", str(scratch / "bench-target")], check=True)
    return REPO / "target/release/unisphere", scratch / "bench-target/release/status-bench"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", required=True)
    ap.add_argument("--evidence", default=str(Path(__file__).with_name("status-real-evidence.json")))
    a = ap.parse_args()
    scratch = (REPO / a.scratch).resolve()
    scratch.mkdir(parents=True, exist_ok=True)
    binary, bench = build(scratch)
    ev, failures = {}, []

    # 1. largest transcript by id, and a live pij seat by --pij.
    files = sorted(transcripts(), key=lambda p: p.stat().st_size, reverse=True)
    big = files[0]
    code, wall, result = status(binary, ["--session", big.stem, "--harness", "claude-code"])
    ev["largest_by_session"] = {"bytes": big.stat().st_size, "exit": code, "wall_ms": round(wall),
                                "ok": result.get("ok"), "calls": result.get("status", {}).get("calls", {}).get("total")}
    if code != 0 or wall > 3000:
        failures.append("largest_by_session")
    seats = json.loads(subprocess.run(["pij", "list", "--json"], capture_output=True, text=True).stdout)["data"]
    seats = seats if isinstance(seats, list) else next(v for v in seats.values() if isinstance(v, list))
    by_size = []
    for seat in seats:
        if seat.get("harness") == "claude" and seat.get("session"):
            hit = next(iter(PROJECTS.glob(f"*/{seat['session']}.jsonl")), None)
            if hit:
                by_size.append((hit.stat().st_size, seat["id"]))
    if by_size:
        size, seat = max(by_size)
        code, wall, result = status(binary, ["--pij", seat])
        ev["largest_seat_by_pij"] = {"bytes": size, "exit": code, "wall_ms": round(wall), "ok": result.get("ok"),
                                     "basis": result.get("status", {}).get("resolved", {}).get("basis")}
        if code != 0:
            failures.append("largest_seat_by_pij")

    # 2. concurrent appender on a scratch copy (never the native store).
    home = scratch / "home"
    copy = home / ".claude/projects/proof" / big.name
    copy.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(big, copy)
    lines = big.read_bytes().splitlines(keepends=True)[-200:]
    stop = threading.Event()

    def appender():
        with open(copy, "ab", buffering=0) as f:
            i = 0
            while not stop.is_set():
                line = lines[i % len(lines)]
                cut = max(1, len(line) // 3)
                f.write(line[:cut]); time.sleep(0.003); f.write(line[cut:])
                i += 1
    env = dict(os.environ, HOME=str(home))
    thread = threading.Thread(target=appender); thread.start()
    runs = [status(binary, ["--session", big.stem, "--harness", "claude-code"], env=env) for _ in range(10)]
    stop.set(); thread.join()
    ev["live_append"] = {"runs": len(runs), "all_exit_0": all(r[0] == 0 for r in runs),
                         "pending_tail_seen": sum(1 for r in runs if r[2].get("status", {}).get("source", {}).get("pending_tail_bytes", 0) > 0),
                         "median_wall_ms": round(statistics.median(r[1] for r in runs))}
    if not ev["live_append"]["all_exit_0"]:
        failures.append("live_append")

    # 3. 50 most recent transcripts, warm with caller-held cursors.
    recent = sorted(transcripts(), key=lambda p: p.stat().st_mtime, reverse=True)[:50]
    p, _ = run([str(bench), "warm", str(PROJECTS), *map(str, recent)], timeout=1800)
    warm = json.loads(p.stdout)
    warm["total_bytes"] = sum(f.stat().st_size for f in recent)
    warm["warm_ms"] = [round(x, 2) for x in warm["warm_ms"]]
    warm["cold_ms"] = round(warm["cold_ms"])
    ev["fifty_seats"] = warm
    if warm["failed"] or statistics.median(warm["warm_ms"]) >= 100:
        failures.append("fifty_seats")

    # 4. native pane lookup, pij removed from PATH.
    pij_dir = str(Path(shutil.which("pij")).parent) if shutil.which("pij") else None
    path = os.pathsep.join(d for d in os.environ["PATH"].split(os.pathsep) if d != pij_dir)
    panes = [s["pane"] for s in seats if s.get("harness") == "claude" and s.get("pane")][:5]
    p, _ = run([str(bench), "panes", *panes], env=dict(os.environ, PATH=path))
    pane = json.loads(p.stdout) if p.returncode == 0 else []
    resolved = [x for x in pane if x["ok"]]
    ev["native_pane"] = {"panes": len(pane), "resolved": len(resolved),
                         "median_ms": round(statistics.median(x["median_ms"] for x in resolved), 1) if resolved else None}
    # Jordan 2026-09-30: "about 50 ms, limited by one ps call" (ps alone ~44 ms).
    if not resolved or ev["native_pane"]["median_ms"] > 75:
        failures.append("native_pane")

    # 5. a real model switch reports the newest model.
    switched = None
    for f in sorted(transcripts(), key=lambda p: p.stat().st_size):
        if f.stat().st_size < 50_000_000 and b"Set model to" in f.read_bytes():
            code, _, result = status(binary, ["--session", f.stem, "--harness", "claude-code"])
            history = result.get("status", {}).get("model", {}).get("history", [])
            if code == 0 and len({h["model"] for h in history}) >= 2:
                current = result["status"]["model"]["current"]["value"]
                switched = {"models": len({h["model"] for h in history}), "current_is_newest": current == history[-1]["model"]}
                break
    ev["model_switch"] = switched
    if not switched or not switched["current_is_newest"]:
        failures.append("model_switch")

    ev["failures"] = failures
    Path(a.evidence).write_text(json.dumps(ev, indent=1) + "\n")
    print(json.dumps(ev, indent=1))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
