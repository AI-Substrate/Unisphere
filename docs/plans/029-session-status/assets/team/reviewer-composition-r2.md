# Reviewer packet — Plan 029 composition review, round 2

**Reviewer** `pij-easy-seahorse`. Write only `docs/plans/029-session-status/assets/reviews/composition-r2.md` + `composition-r2.receipt.dd.json` (id `rv-029-composition-r2`, same rules as r1). Subject: the commit adding this file; code at its parent `a57491d90ce6038962b6c68d8a315390e6130697`.

r1 dispositions (see plan § Implementation Summary):
- F-01 fixed: `is_plain_component` gate at the top of `status_incremental` (crates/sdk/src/status.rs) + test `a_session_id_that_is_not_one_path_component_is_refused_before_any_read` (../, .., ., empty, a/b, a\\b, proj/../outside).
- F-02 fixed: crates/cli/docs/session-status.md window row states the 200k/1M ambiguity and GPT windows unknown.
- F-03 accepted (reason in summary). F-04 accepted; stated in the PR description.

Confirm and return verdict: `pij send pij-specific-kiwi "<receipt path>"`.
