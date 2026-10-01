# Third-party notices

Unisphere's source is MIT licensed; see [LICENSE](LICENSE).

No Flowspace3 or git-ai source is vendored in this foundation. Research into their
architecture informed the ports/adapters, configuration and proof approach.
Future substantial source reuse must retain the applicable upstream copyright,
licence and provenance notices; research influence is not a claim of code reuse.

Git-ai-format Git Notes support is independently implemented from the documented
`authorship/3.0.0` input format. No Git AI source, executable, crate/library,
private cache or service is a dependency. The concrete loader invokes only the
caller-selected standard Git binary; synthetic fixtures are authored locally.

## Direct Rust dependencies

These versions and licence expressions were read from the composed
`cargo metadata --locked --format-version 1` result. Upstream projects carry their
licence texts and copyright notices. Cargo resolves further transitive packages
in `Cargo.lock`; this table is not a replacement for their licence obligations.

| Dependency | Locked version | Licence | Upstream |
| --- | --- | --- | --- |
| clap | 4.6.6 | MIT OR Apache-2.0 | <https://github.com/clap-rs/clap> |
| serde | 1.0.229 | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | <https://github.com/serde-rs/json> |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | <https://github.com/Stebalien/tempfile> |
| time | 0.3.55 | MIT OR Apache-2.0 | <https://github.com/time-rs/time> |
| libc | 0.2.189 | MIT OR Apache-2.0 | <https://github.com/rust-lang/libc> |

`tempfile` and `unisphere-testkit` support development proof; the installed
application does not depend on testkit. Node, DD, Builder and native agent
harnesses are development infrastructure, not linked product dependencies.

This delivery makes no binary redistribution or package-registry publication
claim. Before distributing a bundled binary or vendored dependency source,
include the relevant upstream licence and copyright notices.
