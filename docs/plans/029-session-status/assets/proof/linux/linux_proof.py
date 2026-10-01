#!/usr/bin/env python3
"""Plan 029 Linux proof (P-L), run inside the ubuntu:24.04 image by run.sh.

Numbers only. Everything is synthetic and lives under a scratch HOME:
  1. cargo test for core, sdk, cli, loader-query and app: pass/fail counts
  2. tmux pane whose shell forks a long-running stand-in harness; the
     stand-in's pid has ~/.claude/sessions/<pid>.json naming the pane and a
     transcript under ~/.claude/projects
     - `sessions status --pane %N` (no pij on PATH) → native_pane
     - `sessions status --session ID --harness claude-code` → explicit
     - a fake `pij list --json` whose proc_start is packed from procps
       `ps -o lstart=` exactly as Pij does → `--pij` live seat resolves,
       a seat with a recycled start time is a dead binding
     - `--pane` for a pane tmux does not list → UNI-STATUS-PANE-NOT-FOUND
  3. native pane lookup wall time (whole CLI run, median of 20)
"""

import json, os, re, statistics, subprocess, sys, tempfile, time, uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

REPO = Path("/src")
PACKAGES = ["unisphere-core", "unisphere-sdk", "unisphere-cli", "unisphere-loader-query", "unisphere-app"]
MONTHS = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split()


def sh(argv, env=None, check=False):
    return subprocess.run(argv, capture_output=True, text=True, env=env, check=check)


def cargo_tests():
    counts = {}
    for package in PACKAGES:
        p = sh(["cargo", "test", "--locked", "-p", package])
        passed = sum(int(n) for n in re.findall(r"test result: \w+\. (\d+) passed", p.stdout))
        failed = sum(int(n) for n in re.findall(r"(\d+) failed", " ".join(re.findall(r"test result:[^\n]*", p.stdout))))
        counts[package] = {"exit": p.returncode, "passed": passed, "failed": failed}
    return counts


def pack_lstart(text):
    """Pij's packing of C-locale `ps -o lstart=`: YYYYMMDDhhmmss."""
    _, month, day, clock, year = text.split()
    h, m, s = clock.split(":")
    return int(f"{year}{MONTHS.index(month) + 1:02}{int(day):02}{h}{m}{s}")


def lstart(pid):
    return sh(["ps", "-o", "lstart=", "-p", str(pid)], env={**os.environ, "LC_ALL": "C"}).stdout.strip()


def status(binary, args, env):
    t = time.perf_counter()
    p = sh([binary, "sessions", "status", *args, "--json"], env=env)
    wall = (time.perf_counter() - t) * 1000
    try:
        result = json.loads(p.stdout)["data"]["results"][0]
    except (ValueError, KeyError, IndexError):
        result = {}
    return p.returncode, wall, result


def summary(code, result):
    resolved = result.get("status", {}).get("resolved") or result.get("resolved") or {}
    return {
        "exit": code,
        "ok": result.get("ok") is True,
        "basis": resolved.get("basis"),
        "conflicts": len(resolved.get("conflicts") or []),
        "code": (result.get("error") or {}).get("code"),
    }


def main():
    evidence = {"tests": cargo_tests()}
    subprocess.run(["cargo", "build", "--locked", "--release", "-q", "-p", "unisphere-app"], check=True)
    binary = str(Path(os.environ["CARGO_TARGET_DIR"]) / "release" / "unisphere")
    evidence["tools"] = {
        "procps": sh(["ps", "--version"]).stdout.split()[-1],
        "tmux": sh(["tmux", "-V"]).stdout.split()[-1],
    }

    home = Path(tempfile.mkdtemp(prefix="p-l-home-"))
    bin_dir = home / "bin"
    bin_dir.mkdir()
    env = {"HOME": str(home), "PATH": "/usr/bin:/bin", "TERM": "xterm", "TMUX_TMPDIR": str(home)}
    session = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    stamp = lambda seconds: (now - timedelta(seconds=seconds)).strftime("%Y-%m-%dT%H:%M:%S.000Z")
    project = home / ".claude/projects/-work-proof"
    project.mkdir(parents=True)
    base = {"sessionId": session, "cwd": "/work/proof", "isSidechain": False}
    lines = [
        {"type": "user", "timestamp": stamp(30), **base, "message": {"role": "user", "content": "synthetic"}, "origin": {"kind": "human"}},
        {"type": "assistant", "timestamp": stamp(20), **base, "requestId": "req_1", "message": {
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-proof-1",
            "content": [{"type": "text", "text": "synthetic"}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "cache_read_input_tokens": 0, "output_tokens": 5,
                      "cache_creation": {"ephemeral_1h_input_tokens": 100, "ephemeral_5m_input_tokens": 0}}}},
    ]
    (project / f"{session}.jsonl").write_text("".join(json.dumps(line) + "\n" for line in lines))

    # The pane's shell forks the stand-in harness (the trailing `:` stops sh exec-ing it).
    sh(["tmux", "new-session", "-d", "-s", "proof", "sh -c 'sleep 600; :'"], env=env, check=True)
    time.sleep(0.5)
    pane, pane_pid = sh(["tmux", "list-panes", "-t", "proof", "-F", "#{pane_id} #{pane_pid}"], env=env).stdout.split()
    harness_pid = int(sh(["ps", "-o", "pid=", "--ppid", pane_pid]).stdout.split()[0])
    sessions = home / ".claude/sessions"
    sessions.mkdir(parents=True)
    (sessions / f"{harness_pid}.json").write_text(json.dumps({
        "pid": harness_pid, "sessionId": session, "cwd": "/work/proof", "tmux": f"proof:@0.{pane}",
    }))

    code, _, result = status(binary, ["--pane", pane], env)
    evidence["pane_native"] = summary(code, result)
    evidence["pane_native"]["session_matches"] = (result.get("status") or {}).get("target", {}).get("session_id") == session
    code, _, result = status(binary, ["--session", session, "--harness", "claude-code"], env)
    evidence["session_explicit"] = summary(code, result)
    evidence["session_explicit"]["has_model"] = bool(json.dumps(result).count("claude-proof-1"))
    code, _, result = status(binary, ["--pane", "%999"], env)
    evidence["pane_missing"] = summary(code, result)

    walls = [status(binary, ["--pane", pane], env)[1] for _ in range(20)]
    evidence["pane_native_wall_ms"] = {"runs": 20, "median": round(statistics.median(walls), 1), "max": round(max(walls), 1)}

    # Fake Pij: proc_start packed from procps lstart exactly as Pij records it.
    started = pack_lstart(lstart(harness_pid))
    seat = {"id": "pij-proof-live", "harness": "claude", "session": session, "pane": pane,
            "proc": {"pid": harness_pid, "proc_start": started}, "last_event_at": 2}
    recycled = {**seat, "id": "pij-proof-dead", "pane": None, "proc": {"pid": harness_pid, "proc_start": started - 1}, "last_event_at": 1}
    listing = json.dumps({"ok": True, "command": "list", "v": 1, "data": {"seats": [seat, recycled]}})
    (home / "pij-list.json").write_text(listing)
    pij = bin_dir / "pij"
    pij.write_text(f"#!/bin/sh\n[ \"$*\" = 'list --json' ] || exit 2\ncat '{home}/pij-list.json'\n")
    pij.chmod(0o700)
    pij_env = {**env, "PATH": f"{bin_dir}:{env['PATH']}"}
    code, _, result = status(binary, ["--pij", "pij-proof-live"], pij_env)
    evidence["pij_live"] = summary(code, result)
    code, _, result = status(binary, ["--pij", "pij-proof-dead"], pij_env)
    evidence["pij_recycled_start"] = summary(code, result)
    code, _, result = status(binary, ["--pane", pane], pij_env)
    evidence["pane_pij_and_native"] = summary(code, result)

    sh(["tmux", "kill-server"], env=env)
    expected = {
        "pane_native": ("native_pane", None),
        "session_explicit": ("explicit", None),
        "pane_missing": (None, "UNI-STATUS-PANE-NOT-FOUND"),
        "pij_live": ("pij_registry", None),
        "pij_recycled_start": (None, "UNI-STATUS-DEAD-BINDING"),
        "pane_pij_and_native": ("pij_registry", None),
    }
    failures = [name for name, (basis, code) in expected.items()
                if (evidence[name]["basis"], evidence[name]["code"]) != (basis, code)
                or evidence[name]["conflicts"] != 0]
    failures += [name for name, counts in evidence["tests"].items() if counts["exit"] != 0 or counts["failed"]]
    if not evidence["pane_native"]["session_matches"] or not evidence["session_explicit"]["has_model"]:
        failures.append("targets")
    evidence["failures"] = failures
    print(json.dumps(evidence, indent=1))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
