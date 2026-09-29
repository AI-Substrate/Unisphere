# Security policy

## Reporting a vulnerability

Please report security issues privately, not in public issues or pull requests.

Use GitHub's private reporting: open the repository's **Security** tab and choose **Report a vulnerability**. Include the affected version or commit, steps to reproduce, and the impact you observed.

We aim to acknowledge reports within five working days and will keep you informed while we investigate and fix the issue.

## Scope

Unisphere reads agent-session stores on the local machine, read-only. Issues of particular interest:

- reading, writing or following paths outside the explicitly selected sources or target directory;
- leaking conversation content when content output was not explicitly requested;
- command or option injection through external tools Unisphere invokes (Git, Pij);
- unbounded resource use from crafted input.

## Supported versions

Unisphere is pre-1.0. Security fixes land on `main`; there are no maintained release branches yet.
