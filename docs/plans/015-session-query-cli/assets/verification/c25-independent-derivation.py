#!/usr/bin/env python3
"""Independent pre-execution derivation for the fixed C25 query-view vector.

This uses only Python's standard-library JSON and SHA-256 implementations. It
does not invoke, import, parse, or scrape the Rust implementation or test.
"""

import hashlib
import json
from collections import OrderedDict as O


def framed_sha256(domain: bytes, *components: bytes) -> str:
    preimage = len(domain).to_bytes(8, "little") + domain
    for component in components:
        preimage += len(component).to_bytes(8, "little") + component
    return hashlib.sha256(preimage).hexdigest()


source_digest = framed_sha256(
    b"unisphere/query-source/v1",
    b"fixture-source",
)
source_id = f"q1:source:{source_digest}"

basis = O([
    ("admitted_repository_roots", ["/fixtures/project"]),
    ("admitted_scope", O([
        ("kind", "repository"),
        ("path", "/fixtures/project"),
        ("scope", "exact"),
    ])),
    ("input", O([("kind", "live_native")])),
    ("reconstruction_version", 1),
    ("retained", O([
        ("access", O([
            ("emit_content", False),
            ("inspect_fields", []),
        ])),
        ("fields_by_source", O([
            (source_id, ["id", "source_refs"]),
        ])),
    ])),
    ("selected_associations", []),
    ("source_read_facts", O([
        ("association_status", O([("matched", 1)])),
        ("discovered_sources", 1),
        ("excluded_adapters", []),
        ("issues", []),
        ("loaded_sources", 1),
        ("selected_sources", 1),
        ("source_read_complete", True),
        ("source_status", O([("readable", 1)])),
    ])),
    ("source_selection", O([
        ("exclude_adapters", []),
        ("exclude_harnesses", []),
        ("include_adapters", []),
        ("include_harnesses", []),
    ])),
    ("sources", [O([
        ("query_policy_version", "policy-v1"),
        ("representation", "claude-jsonl-v1"),
        ("revision", "revision-a"),
        ("source_id", source_id),
    ])]),
    ("view_schema_version", 1),
])

canonical_json = json.dumps(
    basis,
    ensure_ascii=False,
    separators=(",", ":"),
).encode("utf-8")
domain = b"unisphere/query-view/v1\0"
preimage = domain + canonical_json
digest = hashlib.sha256(preimage).hexdigest()

assert source_id == (
    "q1:source:"
    "aea024db164980dee299775e2ca50ec8c6cbb4a0607aa1300f84bb46951e678b"
)
assert len(preimage) == 972
assert digest == "23c71eb09affec419f8bf44180fcb9b975d9ead38351be921688a1b904d0ae35"

print(f"source_id={source_id}")
print(f"domain_hex={domain.hex()}")
print(f"canonical_json={canonical_json.decode('utf-8')}")
print(f"preimage_hex={preimage.hex()}")
print(f"preimage_bytes={len(preimage)}")
print(f"sha256={digest}")
