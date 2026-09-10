//! Version-matched offline documentation bundled into the CLI binary.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocTopic {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) text: &'static str,
    pub(crate) related: &'static [&'static str],
}

static TOPICS: &[DocTopic] = &[
    DocTopic {
        id: "start",
        title: "Start querying agent evidence",
        summary: "Choose an explicit scope, discover evidence, and follow a safe next step.",
        text: include_str!("../docs/start.md"),
        related: &["agents", "find-sessions", "privacy-and-coverage"],
    },
    DocTopic {
        id: "agents",
        title: "Using Unisphere from an agent",
        summary: "Stable machine output, clean streams, bindings, and outcome-specific actions.",
        text: include_str!("../docs/agents.md"),
        related: &["start", "output-and-schema", "troubleshooting"],
    },
    DocTopic {
        id: "find-sessions",
        title: "Find repository-associated sessions",
        summary: "Discover source coverage before selecting a session.",
        text: include_str!("../docs/find-sessions.md"),
        related: &["inspect-conversations", "privacy-and-coverage", "git-ai"],
    },
    DocTopic {
        id: "inspect-conversations",
        title: "Inspect conversations and lineage",
        summary: "Move from a session to branches, turns, messages, tools, and events.",
        text: include_str!("../docs/inspect-conversations.md"),
        related: &["find-sessions", "extract-context", "tool-analysis"],
    },
    DocTopic {
        id: "filter-time-and-text",
        title: "Filter by time, metadata, and text",
        summary: "Apply typed filters without confusing unknown evidence with a match.",
        text: include_str!("../docs/filter-time-and-text.md"),
        related: &[
            "output-and-schema",
            "extract-context",
            "privacy-and-coverage",
        ],
    },
    DocTopic {
        id: "extract-context",
        title: "Extract review context",
        summary: "Emit bounded, branch-qualified context with explicit content consent.",
        text: include_str!("../docs/extract-context.md"),
        related: &[
            "inspect-conversations",
            "filter-time-and-text",
            "output-and-schema",
        ],
    },
    DocTopic {
        id: "tool-analysis",
        title: "Analyse tool outcomes and durations",
        summary: "Separate observed failures, incomplete calls, and measured timing.",
        text: include_str!("../docs/tool-analysis.md"),
        related: &[
            "inspect-conversations",
            "filter-time-and-text",
            "privacy-and-coverage",
        ],
    },
    DocTopic {
        id: "output-and-schema",
        title: "Output formats and schema discovery",
        summary: "Select a format, inspect field capabilities, and preserve clean streams.",
        text: include_str!("../docs/output-and-schema.md"),
        related: &["agents", "privacy-and-coverage", "troubleshooting"],
    },
    DocTopic {
        id: "privacy-and-coverage",
        title: "Privacy, provenance, and coverage",
        summary: "Understand content consent, partial reads, and evidence boundaries.",
        text: include_str!("../docs/privacy-and-coverage.md"),
        related: &["find-sessions", "output-and-schema", "troubleshooting"],
    },
    DocTopic {
        id: "sdk",
        title: "Rust SDK query recipes",
        summary: "Use injected query ports and the same typed schemas as the CLI.",
        text: include_str!("../docs/sdk.md"),
        related: &["agents", "output-and-schema", "privacy-and-coverage"],
    },
    DocTopic {
        id: "troubleshooting",
        title: "Error recovery",
        summary: "Map stable error codes to safe, cause-specific recovery.",
        text: include_str!("../docs/troubleshooting.md"),
        related: &["start", "output-and-schema", "privacy-and-coverage"],
    },
    DocTopic {
        id: "git-ai",
        title: "Git-AI attribution evidence",
        summary: "Relate registered Git note attribution to sessions without inventing transcripts.",
        text: include_str!("../docs/git-ai.md"),
        related: &[
            "find-sessions",
            "inspect-conversations",
            "privacy-and-coverage",
        ],
    },
];

pub(crate) const fn topics() -> &'static [DocTopic] {
    TOPICS
}

pub(crate) fn get(id: &str) -> Option<&'static DocTopic> {
    TOPICS.iter().find(|topic| topic.id == id)
}
