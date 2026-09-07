# Original ask — SDK and CLI foundation

**Captured:** 2026-09-07 · **By:** pij-female-varl

> okay so plan 1 will be to get the sdk and cli in. probably can pinch a bunch of stuff from flowspace there. too. lets get a /builder plan on it. primes always write the intial plan then hav the pms orchestrate implemtajtions with their peers.

Scope clarification offered foundation-only delivery versus foundation plus the first native reader; Jordan selected:

> Foundation only

That option meant: public Rust SDK, thin CLI, real shared configuration/diagnostic behavior, dependency boundaries, packaging and tests; telemetry format and native adapters remain separate work.

Jordan's parallel-construction requirement:

> then while that plan is being impleennted we can get coders and other agents working on bits of it... this is the whole sctick of the arhictecurer, we can build services etc separetlin in agents and then compose them after, faning out. no need to wait for cli, we can assume how it will work and contirnie on building in parallel.

Interpretation carried into planning: independent components depend on agreed contracts rather than finished sibling implementations; the prime owns initial product intent and the PM owns later peer orchestration/composition. This is not permission to invent incompatible interfaces or dispatch before the reviewed guide/current baseline requirements are met.

Prior requirements, research and the pending first-consumer interview remain linked from `requirements-spine.md`.
