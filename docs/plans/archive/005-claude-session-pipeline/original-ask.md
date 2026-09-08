# Original ask — Claude session pipeline

Jordan requested a first client adapter (probably Claude Code), the output-side contract and easy addition of further adapters, then clarified:

> adaptors shoudl not have file system access, they shoudl just be provided the data and return a contracted result. this way they aer super unit testable. Or, maybe htey get provided a session loader that can be faked in tests... thought?

The agreed design is a shared injectable SessionLoader with list_sessions and read_batch; pure adapters receive provided data/state and return contracted results; storage and output I/O remain outside adapters. Claude and other JSONL clients may share one loader.

Jordan approved the numbered process: new Builder worktree from main; this PM writes plan and implementation guide; fresh OMP Claude Opus5/high reviewer; three fresh OMP GitHub Astra/high coder lanes for loader, adapter and output; PM composition; full proof and independent review; post-flight/archive; later main landing with approval. Response: **go**.

Standing operator direction: Builder is dogfood, not a product blocker; preserve real proof and report actual tool friction, use agreed workarounds instead of pointless ceremony. Ask material user questions one at a time, one context sentence and one ask sentence. No private stores, remote publication, global changes or workspace retirement authorized here.
