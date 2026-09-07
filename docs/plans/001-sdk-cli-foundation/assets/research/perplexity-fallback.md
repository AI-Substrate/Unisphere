# Perplexity fallback — source-discovery evidence

**Status:** Successful fast answer, not the deep-research result.
**Date:** 2026-09-07.
**Tool reported by research agent:** `xd://mcp__perplexity_ask`, `search_context_size: low`; prior research/high-context MCP calls timed out at the stdio boundary.

## Prompt

> Is there an official OpenTelemetry standard on-disk file format for AI agent session history or telemetry, or is OTLP JSONL solely an exporter convention? Summarize status of semantic-conventions-genai, file exporter framing, and session replay limitations.

## Returned answer, preserved as research evidence

> No. **OpenTelemetry does not define an official, portable on-disk format for AI-agent session history or session replay.** OTLP JSONL is an official **File Exporter wire/storage encoding for telemetry**, but it is not an agent transcript, event-sourcing, or replay format.[4]
>
> ## Status by area
>
> - **`semantic-conventions-genai`** defines names, attributes, events, and relationships for GenAI workloads—including agents and frameworks—but it specifies telemetry meaning, not how sessions are serialized to disk. The agent conventions cover spans such as agent creation and agent activity; they do not define a canonical “session-history file.”[1][2]
> - **Conversation/session correlation** is represented through telemetry attributes such as `gen_ai.conversation.id`, described as an identifier for a conversation, session, or thread. That lets backends group related spans, but an identifier is not a prescribed session container or replay protocol.[7]
> - **The OpenTelemetry File Exporter** prefers the `.jsonl` extension and provides a file representation for exported OTLP data.[4] Thus, “OTLP JSONL” is more than an arbitrary vendor convention when used by that exporter, but it is still an exporter-level telemetry format—not a universal OpenTelemetry agent-history standard.
> - **Session replay is consequently limited.** A collector or backend can reconstruct an approximate timeline by correlating spans, events, IDs, timestamps, links, and recorded content, but OTLP does not guarantee a complete, lossless, deterministic replay of an agent run. Prompt and response bodies, tool arguments/results, internal state, memory contents, branching decisions, and intermediate context may be absent, redacted, sampled, or represented differently by instrumentation.
> - **Agent semantics are still evolving.** The dedicated GenAI conventions repository contains agent/framework conventions, while the OpenTelemetry documentation points users to that repository for the moved GenAI semantic conventions.[1][2] These conventions should therefore be treated as telemetry conventions rather than a finalized archival/session model.
>
> In short: **OTLP JSONL can be used as a practical telemetry dump, and `gen_ai.conversation.id` can provide session grouping, but neither constitutes an official OpenTelemetry on-disk AI-agent session format or replay guarantee.**

## Returned citations

1. https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-agent-spans.md
2. https://opentelemetry.io/docs/specs/semconv/gen-ai/
3. https://docs.honeycomb.io/send-data/use-cases/agents
4. https://opentelemetry.io/docs/specs/otel/protocol/file-exporter/
5. https://greptime.com/blogs/2026-05-09-opentelemetry-genai-semantic-conventions
6. https://github.com/open-telemetry/semantic-conventions-genai/issues/35
7. https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/
8. https://openobserve.ai/blog/opentelemetry-genai-semantic-conventions/
9. https://zylos.ai/research/2026-02-28-opentelemetry-ai-agent-observability/
10. https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-agent-spans/

## Use of this result

The leading "No" answers the full-session/replay question, not whether an official telemetry file serialization exists. Main verified that the answer to the latter is **yes**, at Development status, in the dedicated File Exporter specification. Findings and corrections are in [otel-common-format.md](otel-common-format.md); the references above are returned Perplexity citations, not a claim that every listed secondary page was independently checked.
