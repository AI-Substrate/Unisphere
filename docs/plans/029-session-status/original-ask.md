# Original ask — session status

Jordan, 2026-09-29, in order:

> you will be adding a general status for a session ... a feature that allows agents to give a session id + agent harness or just a pij id and get status like context size (x of y like 250k of 1m) and percentage, number of times compacted, time since last updated, time created, total turns etc... just general facts.

> note the power of cross agent canonical formats so a caller can check any agent without having to know about that particular agent's inners

> also, given a tmux window id like %0 ... we should be able to instantly map that to agent harness and session id and pij id within a blink of an eye

> needs current model, sometimes i change them and i've seen it get original model out of data.

> get a plan written on a worktree, you will be pm, omp harness opus 5.5 github copilot based model coder, sonnet 5.5 based reviewer. sonnet 5.5 is coming when pij-curly-kiwi finishes its work

> KISS, dont over bake it all just get it done, its simple ask

Inputs (read, do not re-derive):

- Prime scope ruling + pane ruling: `/Users/jordanknight/substrate/unisphere/unishpere-main/scratch/session-status-prime-answers.md`
- pij send-pricing consumer (pij-far-jackal) and kiwi's answers: `/Users/jordanknight/substrate/unisphere/unishpere-main/scratch/session-status-far-jackal-answers.md`; RCA `~/games/unasphere/scratch/usage-blowout/far-jackal-interview.md` §2
- Base fold (Plan 028, branch `builder/028-prep-canonical-tables`): `crates/core/src/prep.rs`, `crates/adapter-claude/src/prep.rs`, `crates/sdk/src/prep.rs`
