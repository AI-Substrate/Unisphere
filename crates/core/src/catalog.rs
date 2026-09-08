//! Static application metadata; location hints never perform discovery.
use serde::Serialize;

/// Description of an adapter actually registered by a composition root.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AdapterDescriptor {
    /// Stable selection ID, also used in exported source provenance.
    pub id: &'static str,
    /// Application producing the native session records.
    pub application: &'static str,
    /// Scope of this adapter's projection.
    pub description: &'static str,
    /// Symbolic usual locations, not observations of this machine.
    pub locations: &'static [LocationHint],
    /// Mechanisms and limitations of the registered collection pipeline.
    pub capabilities: AdapterCapabilities,
}

/// A declarative hint; consumers choose whether and where to discover files.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct LocationHint {
    /// Platforms on which this usual location applies.
    pub platforms: &'static [&'static str],
    /// Symbolic base such as `home`; never expanded by the catalog.
    pub base: &'static str,
    /// Path relative to the symbolic base.
    pub path: &'static str,
    /// Session pattern relative to `path`; never evaluated by the catalog.
    pub session_glob: &'static str,
    /// Native storage format, distinct from the exported format.
    pub storage_format: &'static str,
}

/// Explicit capabilities, not installation detection or a completeness claim.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AdapterCapabilities {
    /// Platforms supported by the registered loader/export pipeline.
    pub export_platforms: &'static [&'static str],
    /// Formats the registered writer can emit.
    pub output_formats: &'static [&'static str],
    /// Whether the SDK returns a cursor for the caller to retain.
    pub sdk_caller_owned_cursor: bool,
    /// Source mutation assumption required when reusing that cursor.
    pub cursor_source_assumption: &'static str,
    /// Whether the CLI persists and restores its own checkpoint.
    pub cli_persisted_resume: bool,
    /// Whether late revisions and deletions are reconciled.
    pub delayed_revision_reconciliation: bool,
    /// Whether all original native data can be reconstructed from the output.
    pub lossless_archive: bool,
}
