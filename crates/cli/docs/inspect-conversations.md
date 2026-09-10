# Inspect conversations and lineage

## Motivating question

What happened in a selected run, and which branch, turn, message, tool, and event evidence supports that account?

## Prerequisites

Start from an exact local session ID returned by the same source view. If lineage has multiple branches, select one returned branch ID instead of using a native ID or choosing the newest timestamp.

## Recipe 2 — follow one conversation without flattening it

```sh
unisphere sessions tree "$SESSION_ID" --repo . --format json
unisphere turns list --repo . --session "$SESSION_ID" --format json
unisphere turns show "$TURN_ID" --repo . --format json
unisphere messages list --repo . --session "$SESSION_ID" --format json
unisphere tools list --repo . --session "$SESSION_ID" --format json
unisphere events list --repo . --session "$SESSION_ID" --format json
```

Expected: turns represent supported initiating request boundaries, not physical records. Tool-result-shaped or injected records do not become extra user turns. Messages and calls link through explicit local IDs. Events preserve observations that cannot justify a higher-level row.

Interpretation: Unisphere supplies versioned reconstruction basis and branch membership. It does not infer ancestry from timestamps, concatenate fork leaves, choose the active branch by recency, or claim that every event is a turn/call/span. A human may narrate the evidence only after retaining those qualifications.

## Creative use cases

- Check whether a copied prefix is represented once with branch-qualified memberships.
- Locate an incomplete tool result through events when call pairing is unavailable.
- Review subagent lineage without following a source-provided path.
- Distinguish native session creation time from first observed event time.

## Limits and recovery

An ambiguous native identity or branch returns candidates. Bind one exact `q1:` ID and retry. Dangling/cyclic/conflicting lineage is reported, not repaired. If a representation lacks reliable turn boundaries, query messages/events rather than treating unavailable turns as an empty conversation.

**Next step:** use `unisphere docs get extract-context` to carry a bounded section into review.
