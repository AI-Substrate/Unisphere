# Native session release — current shipping status

- Branch: `release/native-session-pipeline` → `main`.
- Published release SHA: `4a907d6e2832f3907c6538809b4e07e7d2c7a724`.
- Reviewed product: `c6f3636b394f0e17445eb2e143f6ac2c6d1ec998`; source/manifests/product docs are unchanged by local convergence.
- Local merge authorization: direct user `PROCEED`. Main, Plan005 and Plan009 histories were merged only into the isolated release branch; all required ancestor SHAs and three complete archived plans are retained.
- Push authorization: direct user “yes, do it. no need to ask that”; push succeeded without force.
- PR: not opened yet; exact `gh pr create` command was presented for its separate required confirmation.
- Main checkout remains at `452e0f3cbf4c045a9d5bb10f16f5dc96134a8e98`; its untracked operator files were untouched.

## Actual CI

Run: https://github.com/AI-Substrate/Unisphere/actions/runs/34292699381

Both `macos-latest` and `ubuntu-latest` failed at **Install development harness** with npm `ETARGET`: `@ai-substrate/engineering-harness@0.14.0` is not published. CI did not execute product checks; no CI success is claimed from the prior local results.

A scratch-only installation of published `0.13.0` successfully ran all six quality gates and five boot modes. Main inspected the CI workflow and confirmed that it invokes only those runtime APIs, not Builder/DD. CI is therefore pinned to the published/proved `0.13.0`; the higher local Builder runtime remains separate. The package's runtime dependencies contain no DD Git source, so the speculative `NPM_CONFIG_ALLOW_GIT: all` permission was removed rather than granted unnecessarily. The tooling owner was asked for any concrete API/security/platform contraindication; Builder/DD age alone is not a dependency of this CI surface. No quality gate was removed or weakened, and no product source changed.

Evidence: `ci-prerequisite-failure.json`, `published-harness-compatibility.json`, `release-convergence.json`, `release-validation.json` beside this report.

## Accepted boundaries

Synthetic fixtures and single-host Unix local proof; projections, not a lossless raw archive, retained history, idempotent ingestion or final-session capture. No persisted CLI resume or implicit private-store discovery. Three accepted review advisories remain documented. No new ingestion feature, restart or workspace retirement is authorized.

## Next controlled actions

1. Retain the published 0.13.0 checks/boot compatibility proof and the separate local Builder capability requirement.
2. Publish the CI correction under the authorized release workflow and obtain its actual GitHub CI result.
3. Open the prepared PR only after its required explicit confirmation; report its actual URL.
4. Merge the PR only under the final required typed confirmation, then fast-forward local main without touching operator files.

Telemetry auto-sync was not performed because command-local `HARNESS_NO_TELEMETRY_AUTOSYNC=1` remains in force. Operator product installation is deferred until main.
