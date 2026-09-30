# Original ask — prep canonical tables

Jordan, 2026-09-28/29, in order:

> unisphere should allow an agent to unpick what is going on in the telemetry too, convert it to common format then run a series of commands to find and do research on the contexts, as they are GOLD. tokens are expensive. in this case the other agent is doing an RCA on cost explosion and needs a good way to convert sessions to a usable format

> would duck db help here. look up in flowspace3 research we did already

> lets do a POC. im thinking unisphere is given a target dir to prep. re-run is idempotent and will update and add new records since last etc... then poc duckdb. get a detailed brief from the other agent on their use case and throw in some creative ones of your own and provide a recommendation and include 2 alternatives

> if cli cannot do what SDK can then perhaps CLI needs to be brought up to parity too.

After the POC reported (verdict: build it for real; exact parity with the reference RCA parser; DuckDB external over Parquet recommended):

> yes, omp agent, github claude 5.5 models please for pm and coder, sonnet 5.5 reviewer.

> i will get sonnet 5.5 added, for now just power on thanks

Inputs carried into this plan (read them; do not re-derive):

- POC brief: `/Users/jordanknight/substrate/unisphere/unishpere-main/scratch/poc-prep-duckdb/BRIEF.md`
- POC findings (measured): `/Users/jordanknight/substrate/unisphere/unishpere-main/scratch/poc-prep-duckdb/FINDINGS.md` and runbook `README.md` alongside; POC code is this branch's base (`poc/prep-duckdb` @ `0c4dbdf`).
- Engine research (Perplexity, cited; its DataFusion build-cost claim was measured wrong by the POC): `.../scratch/poc-prep-duckdb/engine-research-perplexity.md`
- Acceptance oracle from the consumer: `/Users/jordanknight/games/unasphere/scratch/unisphere-feedback/{usecase-brief.md,commands.md,README.md,extract.py}` (extract.py writes content-bearing CSVs — run copies outside Git only).
- Founding brief, which already required an incremental "what is new since cursor X" primitive with trustworthy dedupe keys: `/Users/jordanknight/substrate/flowspace/flowspace3/scratch/brief-harness-telemetry-standardiser.md`
- Flowspace3 incremental-seam need statement (a second consumer of the same fold): `/Users/jordanknight/substrate/flowspace/flowspace3/scratch/unisphere-incremental-seam-brief-2026-09-17.md`

Governance record of these rulings: `unisphere-governance/.harness/government/spine.md` (Decisions, 2026-09-28/29).
