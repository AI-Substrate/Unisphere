#!/bin/sh
# Plan 029 Linux proof (P-L): ubuntu:24.04 (procps-ng, tmux) via Docker.
# Re-runnable; the repository is mounted read-only, build output and the cargo
# registry live in named volumes. Writes numbers-only evidence beside this file.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../../../../.." && pwd)
docker build -q -t unisphere-029-linux "$here" >/dev/null
docker run --rm \
  -v "$repo":/src:ro \
  -v unisphere-029-target:/target \
  -v unisphere-029-registry:/opt/cargo/registry \
  -e CARGO_TARGET_DIR=/target \
  unisphere-029-linux \
  python3 /src/docs/plans/029-session-status/assets/proof/linux/linux_proof.py \
  >"$here/linux-evidence.json"
