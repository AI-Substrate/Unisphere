use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    str::FromStr,
};

use clap::{
    Args, ColorChoice, CommandFactory, FromArgMatches, Parser, Subcommand, error::ErrorKind,
};
use unisphere_core::query::{
    AdapterId, ContextWindow, CsvSafety, Dataset, EntityId, FieldId, FieldValue, Filter, HarnessId,
    InclusiveRange, OfflineRef, Operation, OutputFormat, Predicate, QueryFailure, QueryLimits,
    QueryRequest, QueryScope, RecoveryAction, SavedFormat, SortDirection, SortKey, SourceId,
    SourceSelector, TimeWindow, Timestamp, TimestampBasis, schema,
};
use unisphere_core::{
    ReadLimits, SnapshotLimits,
    prep::{
        DEFAULT_ROOT_LABEL, NativeAddress, PrepCompactRequest, PrepOptions, PrepReadLimits,
        PrepRecordRequest, PrepRequest, PrepSourceSet, derived_root_label,
    },
};

use crate::CliContext;

#[derive(Parser)]
#[command(
    name = "unisphere",
    bin_name = "unisphere",
    about = "Query local agent evidence, inspect configuration, and export explicit native sessions",
    after_help = "Start with `unisphere docs get start`. Query commands require an explicit --repo, --source, or --input scope. Native `sessions list --root`, `sessions list --adapter git-ai --repo`, and `sessions export` remain distinct from query operations.",
    disable_version_flag = true,
    disable_help_subcommand = true,
    color = ColorChoice::Never,
    term_width = 96
)]
struct Cli {
    /// Write a versioned JSON command envelope where the command supports it.
    #[arg(long, global = true, conflicts_with = "human")]
    json: bool,
    /// Write readable output and diagnostics.
    #[arg(long, global = true, conflicts_with = "json")]
    human: bool,
    /// Print the application version.
    #[arg(short = 'V', long, global = true)]
    version: bool,
    #[command(subcommand)]
    command: Option<RootCommand>,
}

#[derive(Subcommand)]
enum RootCommand {
    /// Validate explicit configuration without opening source stores.
    #[command(after_help = "Next: unisphere docs get start --human")]
    Config {
        #[command(subcommand)]
        command: ConfigSubcommand,
    },
    /// Describe registered adapters; locations are hints, not detected installations.
    #[command(after_help = "Next: unisphere docs get find-sessions --human")]
    Adapters {
        #[command(subcommand)]
        command: AdapterSubcommand,
    },
    /// Discover and diagnose source representations.
    #[command(after_help = "Workflow: unisphere docs get find-sessions --human")]
    Sources {
        #[command(subcommand)]
        command: SourcesSubcommand,
    },
    /// Find, inspect, measure, and extract sessions.
    #[command(after_help = "Workflow: unisphere docs get inspect-conversations --human")]
    Sessions {
        #[command(subcommand)]
        command: SessionsSubcommand,
    },
    /// Find, inspect, measure, and extract reconstructed turns.
    #[command(after_help = "Workflow: unisphere docs get extract-context --human")]
    Turns {
        #[command(subcommand)]
        command: TurnsSubcommand,
    },
    /// Find, inspect, and extract messages.
    #[command(after_help = "Workflow: unisphere docs get filter-time-and-text --human")]
    Messages {
        #[command(subcommand)]
        command: MessagesSubcommand,
    },
    /// Find, inspect, measure, and extract tool invocations.
    #[command(after_help = "Workflow: unisphere docs get tool-analysis --human")]
    Tools {
        #[command(subcommand)]
        command: ToolsSubcommand,
    },
    /// Find, inspect, and extract source-derived events.
    #[command(after_help = "Workflow: unisphere docs get inspect-conversations --human")]
    Events {
        #[command(subcommand)]
        command: EventsSubcommand,
    },
    /// Inspect the version-matched query schema without source access.
    #[command(after_help = "Guide: unisphere docs get output-and-schema --human")]
    Schema {
        #[command(subcommand)]
        command: SchemaSubcommand,
    },
    /// Incrementally convert native sessions into canonical metadata tables in a target directory.
    #[command(
        after_help = "Roots: catalogue default roots for every harness (or each --harness) unless --no-default-roots, plus every explicit --root HARNESS[:LABEL]=DIR. Re-runs read only records appended since the committed cursor; unchanged sources cost one stat. Tables are Parquet under TARGET/tables; load TARGET/views.sql in DuckDB. Metadata only unless --include-content.\nGuide: unisphere docs get prep --human"
    )]
    Prep(PrepArgs),
    /// Read version-matched offline operating guides.
    #[command(after_help = "Start: unisphere docs get start --human")]
    Docs {
        #[command(subcommand)]
        command: DocsSubcommand,
    },
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
struct PrepArgs {
    #[command(subcommand)]
    command: Option<PrepSubcommand>,
    /// Target directory for state.json, tables/ and views.sql; created if absent.
    #[arg(long, value_name = "DIR", required = true)]
    target: Option<PathBuf>,
    /// Explicit native root (repeatable). LABEL defaults to root-<8 hex of sha256(DIR)>.
    #[arg(long = "root", value_name = "HARNESS[:LABEL]=DIR")]
    roots: Vec<String>,
    /// Restrict the run to this harness (repeatable); explicit roots must match.
    #[arg(long = "harness", value_name = "HARNESS")]
    harnesses: Vec<String>,
    /// Use only explicit --root sets; skip the adapter catalogue's default roots.
    #[arg(long)]
    no_default_roots: bool,
    /// Opt in to content columns (triggers.content_head); absent otherwise.
    #[arg(long)]
    include_content: bool,
    /// Largest physical native record accepted, in bytes (default: the batch limit).
    #[arg(long, value_name = "N")]
    max_record_bytes: Option<usize>,
    /// Native bytes read per bounded batch; at least --max-record-bytes.
    #[arg(long, value_name = "N")]
    max_batch_bytes: Option<usize>,
    /// Largest whole snapshot source (document, journal or database) read, in bytes.
    #[arg(long, value_name = "N")]
    max_snapshot_bytes: Option<usize>,
    /// Most records one snapshot source may contain.
    #[arg(long, value_name = "N")]
    max_snapshot_records: Option<usize>,
    /// Sources read concurrently (at least 1).
    #[arg(long, value_name = "N")]
    threads: Option<usize>,
    /// Expected new native bytes read, folded and committed per wave (at least 1).
    #[arg(long, value_name = "N")]
    max_run_bytes: Option<u64>,
    /// Scope: sources last modified before this RFC 3339 instant are reported skipped, not read.
    #[arg(long, value_name = "RFC3339")]
    modified_since: Option<String>,
}

#[derive(Subcommand)]
enum PrepSubcommand {
    /// Rewrite current-generation rows into fewer parts without changing any canonical view.
    Compact {
        /// Existing prep target directory.
        #[arg(long, value_name = "DIR")]
        target: PathBuf,
    },
    /// Fetch exactly one native record by its address in a prepped source (content; opt-in).
    Record {
        /// Existing prep target directory.
        #[arg(long, value_name = "DIR")]
        target: PathBuf,
        /// Source key `<harness>/<label>/<file>` from the sources table.
        #[arg(long, value_name = "KEY")]
        source: String,
        /// Byte offset of the record (append sources: native_offset).
        #[arg(
            long,
            value_name = "N",
            required_unless_present = "key",
            conflicts_with = "key"
        )]
        offset: Option<u64>,
        /// Snapshot key of the record (snapshot sources: native_key).
        #[arg(long, value_name = "K")]
        key: Option<String>,
        /// Required: the record is native content.
        #[arg(long)]
        include_content: bool,
    },
    /// List the bundled DuckDB research recipes over the canonical views.
    Recipes,
    /// Print one recipe as a DuckDB script: `unisphere prep recipe NAME --target DIR | duckdb`.
    Recipe {
        /// Recipe name from `unisphere prep recipes`.
        #[arg(value_name = "NAME")]
        name: String,
        /// Existing prep target directory.
        #[arg(long, value_name = "DIR")]
        target: PathBuf,
    },
}

#[derive(Subcommand)]
enum ConfigSubcommand {
    /// Validate explicit settings and return the effective configuration.
    Check(CheckArgs),
}

#[derive(Args)]
struct CheckArgs {
    /// Read this JSON file only; relative paths resolve against the supplied cwd.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Replace source roots; repeat to preserve order and duplicates.
    #[arg(
        long = "source-root",
        value_name = "ROOT",
        conflicts_with = "clear_source_roots"
    )]
    source_roots: Option<Vec<String>>,
    /// Explicitly replace source roots with an empty list.
    #[arg(long, conflicts_with = "source_roots")]
    clear_source_roots: bool,
}

#[derive(Subcommand)]
enum AdapterSubcommand {
    /// List registered production adapters.
    List,
}

#[derive(Subcommand)]
enum SourcesSubcommand {
    /// List admitted source representations and their read coverage.
    List(QueryArgs),
    /// Diagnose one exact source identifier or path.
    Check(QueryArgs),
}

#[derive(Subcommand)]
enum SessionsSubcommand {
    /// Query sessions, list a native JSONL root, or list native Git-AI notes.
    List(SessionListArgs),
    /// Show one exact query session, or resolve the latest native session for --pij.
    Show(SessionShowArgs),
    /// Show observed parent, fork, and subagent relationships.
    Tree(ShowArgs),
    /// Aggregate selected sessions.
    Stats(StatsArgs),
    /// Extract selected session rows or content.
    Extract(QueryArgs),
    /// Export one explicit native source as OTLP JSONL.
    Export(NativeExportArgs),
}

#[derive(Subcommand)]
enum TurnsSubcommand {
    List(QueryArgs),
    Show(ShowArgs),
    Stats(StatsArgs),
    Extract(QueryArgs),
}

#[derive(Subcommand)]
enum MessagesSubcommand {
    List(QueryArgs),
    Show(ShowArgs),
    Extract(QueryArgs),
}

#[derive(Subcommand)]
enum ToolsSubcommand {
    List(QueryArgs),
    Show(ShowArgs),
    Stats(StatsArgs),
    Extract(QueryArgs),
}

#[derive(Subcommand)]
enum EventsSubcommand {
    List(QueryArgs),
    Show(ShowArgs),
    Extract(QueryArgs),
}

#[derive(Subcommand)]
enum SchemaSubcommand {
    /// Show fields, operations, formats, and losses for one dataset.
    Show { dataset: String },
}

#[derive(Subcommand)]
enum DocsSubcommand {
    /// List bundled topics.
    List,
    /// Print one bundled topic.
    Get { topic: String },
}

#[derive(Args)]
struct SessionListArgs {
    /// Existing bounded native JSONL leaf directory listing.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Existing native dispatch selector; only Git-AI note listing uses it here.
    #[arg(long)]
    adapter: Option<String>,
    /// Native JSONL root-list bound.
    #[arg(long)]
    max_sessions: Option<usize>,
    #[arg(long)]
    notes_ref: Option<String>,
    #[arg(long)]
    commit: Vec<String>,
    #[arg(long)]
    git_executable: Option<PathBuf>,
    #[arg(long)]
    max_notes: Option<usize>,
    #[arg(long)]
    max_records: Option<usize>,
    #[arg(long)]
    max_note_bytes: Option<usize>,
    #[arg(long)]
    max_total_bytes: Option<usize>,
    #[arg(long)]
    max_listing_bytes: Option<usize>,
    #[arg(long)]
    command_timeout_ms: Option<u64>,
    #[command(flatten)]
    query: QueryArgs,
}

impl SessionListArgs {
    fn has_git_options(&self) -> bool {
        self.notes_ref.is_some()
            || !self.commit.is_empty()
            || self.git_executable.is_some()
            || self.max_notes.is_some()
            || self.max_records.is_some()
            || self.max_note_bytes.is_some()
            || self.max_total_bytes.is_some()
            || self.max_listing_bytes.is_some()
            || self.command_timeout_ms.is_some()
    }

    fn git_limits_invalid(&self) -> bool {
        self.max_notes == Some(0)
            || self.max_records == Some(0)
            || self.max_note_bytes == Some(0)
            || self.max_total_bytes == Some(0)
            || self.max_listing_bytes == Some(0)
            || self.command_timeout_ms == Some(0)
    }
}

#[derive(Args)]
struct NativeExportArgs {
    #[arg(long, default_value = "claude-code")]
    adapter: String,
    /// Explicit native input. Git-AI export may use --repo instead.
    #[arg(long)]
    input: Option<PathBuf>,
    /// Explicit native Git repository for a registered Git-AI exporter.
    #[arg(long)]
    repo: Option<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    include_content: bool,
    #[arg(long)]
    source_format: Option<String>,
    #[arg(long)]
    table: Option<String>,
    #[arg(long)]
    session_id: Option<String>,
    #[arg(long)]
    max_records: Option<usize>,
    #[arg(long)]
    max_record_bytes: Option<usize>,
    #[arg(long)]
    max_batch_bytes: Option<usize>,
    #[arg(long)]
    max_snapshot_bytes: Option<usize>,
    #[arg(long)]
    notes_ref: Option<String>,
    #[arg(long)]
    commit: Vec<String>,
    #[arg(long)]
    git_executable: Option<PathBuf>,
    #[arg(long)]
    max_notes: Option<usize>,
    #[arg(long)]
    max_note_bytes: Option<usize>,
    #[arg(long)]
    max_total_bytes: Option<usize>,
    #[arg(long)]
    max_listing_bytes: Option<usize>,
    #[arg(long)]
    command_timeout_ms: Option<u64>,
}

impl NativeExportArgs {
    fn has_git_options(&self) -> bool {
        self.notes_ref.is_some()
            || !self.commit.is_empty()
            || self.git_executable.is_some()
            || self.max_notes.is_some()
            || self.max_note_bytes.is_some()
            || self.max_total_bytes.is_some()
            || self.max_listing_bytes.is_some()
            || self.command_timeout_ms.is_some()
    }

    fn git_limits_invalid(&self) -> bool {
        self.max_notes == Some(0)
            || self.max_note_bytes == Some(0)
            || self.max_total_bytes == Some(0)
            || self.max_listing_bytes == Some(0)
            || self.command_timeout_ms == Some(0)
    }
}

#[derive(Args)]
struct SessionShowArgs {
    /// Exact local query entity ID returned by Unisphere; omitted only with --pij.
    entity: Option<String>,
    #[command(flatten)]
    query: QueryArgs,
}

#[derive(Args)]
struct ShowArgs {
    /// Exact local query entity ID returned by Unisphere.
    entity: String,
    #[command(flatten)]
    query: QueryArgs,
}

#[derive(Args)]
struct StatsArgs {
    #[command(flatten)]
    query: QueryArgs,
    #[arg(long, value_delimiter = ',')]
    group_by: Vec<String>,
    #[arg(long, visible_alias = "metric", value_delimiter = ',')]
    metrics: Vec<String>,
}

#[derive(Args, Default)]
struct QueryArgs {
    /// Resolve the latest native source/session mapping for this Pij seat ID.
    #[arg(long)]
    pij: Option<String>,
    #[arg(long)]
    repo: Option<PathBuf>,
    #[arg(long)]
    source: Option<PathBuf>,
    #[arg(long)]
    input: Option<PathBuf>,
    /// Framing for --input -; defaults to json and is invalid for files/live scopes.
    #[arg(long)]
    stdin_format: Option<String>,
    #[arg(long, default_value = "tree")]
    repo_scope: String,

    #[arg(long)]
    harness: Vec<String>,
    #[arg(long = "exclude-harness")]
    exclude_harness: Vec<String>,
    #[arg(long = "source-adapter")]
    source_adapter: Vec<String>,
    #[arg(long = "exclude-source-adapter")]
    exclude_source_adapter: Vec<String>,

    #[arg(long)]
    session: Vec<String>,
    #[arg(long)]
    native_id: Vec<String>,
    #[arg(long)]
    model: Vec<String>,
    #[arg(long)]
    name: Vec<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    parent: Vec<String>,
    #[arg(long)]
    role: Vec<String>,
    #[arg(long)]
    turn: Vec<String>,
    #[arg(long)]
    call: Vec<String>,

    #[arg(long)]
    contains: Vec<String>,
    #[arg(long)]
    regex: Vec<String>,
    #[arg(long)]
    ignore_case: bool,

    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    until: Option<String>,
    #[arg(long)]
    time_field: Option<String>,
    #[arg(long)]
    include_undated: bool,

    #[arg(long, value_delimiter = ',')]
    columns: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    sort: Vec<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long)]
    range: Option<String>,
    #[arg(long, default_value_t = 0)]
    context_before: usize,
    #[arg(long, default_value_t = 0)]
    context_after: usize,

    #[arg(long)]
    min_turns: Option<u64>,
    #[arg(long)]
    min_tool_calls: Option<u64>,
    #[arg(long)]
    has_role: Vec<String>,
    #[arg(long)]
    has_tool: Vec<String>,
    #[arg(long)]
    has_tool_family: Vec<String>,
    #[arg(long)]
    has_errors: bool,
    #[arg(long)]
    tool: Vec<String>,
    #[arg(long)]
    tool_family: Vec<String>,
    #[arg(long)]
    status: Vec<String>,
    #[arg(long)]
    exit_code: Vec<i64>,
    #[arg(long)]
    min_duration: Option<f64>,
    #[arg(long)]
    has_duration: bool,
    #[arg(long)]
    command_contains: Vec<String>,
    #[arg(long)]
    kind: Vec<String>,
    #[arg(long)]
    part: Option<String>,

    #[arg(long)]
    include_content: bool,
    #[arg(long)]
    allow_partial: bool,
    #[arg(long, default_value = "reject")]
    unresolved: String,
    #[arg(long, conflicts_with_all = ["json", "human"])]
    format: Option<String>,
    #[arg(long)]
    csv_safety: Option<String>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    max_output_bytes: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Json,
    JsonDiagnostic,
    Human,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigCommand {
    pub config: Option<PathBuf>,
    pub source_roots: Option<Vec<String>>,
    pub clear_source_roots: bool,
    pub mode: OutputMode,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogCommand {
    pub mode: OutputMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocsCommand {
    List { mode: OutputMode },
    Get { topic: String, mode: OutputMode },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaCommand {
    pub dataset: Dataset,
    pub mode: OutputMode,
}

#[derive(Clone, PartialEq)]
pub struct QueryCommand {
    pub request: QueryRequest,
    pub format: OutputFormat,
    pub csv_safety: CsvSafety,
    pub output: Option<PathBuf>,
    pub diagnostic_mode: OutputMode,
    pub stdin_format: SavedFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PijTarget {
    Query,
    SessionShow,
    SourceCheck,
}

#[derive(Clone, PartialEq)]
pub struct PijQueryCommand {
    pub id: String,
    pub query: QueryCommand,
    pub target: PijTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRootListCommand {
    pub root: PathBuf,
    pub max_sessions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeGitNotesListCommand {
    pub adapter: String,
    pub repo: PathBuf,
    pub notes_ref: String,
    pub commits: Vec<String>,
    pub git_executable: Option<PathBuf>,
    pub max_notes: usize,
    pub max_records: usize,
    pub max_note_bytes: usize,
    pub max_total_bytes: usize,
    pub max_listing_bytes: usize,
    pub command_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeExportCommand {
    pub adapter: String,
    pub input: Option<PathBuf>,
    pub repo: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub include_content: bool,
    pub source_format: Option<String>,
    pub table: Option<String>,
    pub session_id: Option<String>,
    pub max_records: Option<usize>,
    pub max_record_bytes: Option<usize>,
    pub max_batch_bytes: Option<usize>,
    pub max_snapshot_bytes: Option<usize>,
    pub notes_ref: Option<String>,
    pub commits: Vec<String>,
    pub git_executable: Option<PathBuf>,
    pub max_notes: Option<usize>,
    pub max_note_bytes: Option<usize>,
    pub max_total_bytes: Option<usize>,
    pub max_listing_bytes: Option<usize>,
    pub command_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpCommand {
    pub text: String,
    pub mode: OutputMode,
}

/// One explicit `--root HARNESS[:LABEL]=DIR` set. `label: None` derives
/// [`derived_root_label`] from the absolute directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepRoot {
    pub harness: String,
    pub label: Option<String>,
    pub root: PathBuf,
}

impl PrepRoot {
    /// The source set this explicit root contributes to a [`PrepRequest`].
    pub fn source_set(&self) -> PrepSourceSet {
        PrepSourceSet {
            harness: self.harness.clone(),
            label: self
                .label
                .clone()
                .unwrap_or_else(|| derived_root_label(&self.root)),
            root: self.root.clone(),
        }
    }
}

/// Parsed `unisphere prep`. Default-root resolution belongs to the composition
/// root: it adds catalogue roots for [`PrepCommand::wants_default_root`] harnesses
/// unless `no_default_roots`, then appends [`PrepCommand::explicit_sets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepCommand {
    pub target: PathBuf,
    /// Explicit roots in argv order; harness/label and harness/directory are unique.
    pub roots: Vec<PrepRoot>,
    /// `--harness` filters, sorted and unique; empty means every harness.
    pub harnesses: Vec<String>,
    pub no_default_roots: bool,
    pub options: PrepOptions,
    /// Validated read bounds (defaults applied).
    pub limits: PrepReadLimits,
    pub threads: usize,
    /// Commit wave budget in expected new native bytes.
    pub max_run_bytes: u64,
    pub modified_since_ns: Option<i128>,
    pub mode: OutputMode,
}

/// Default `--max-batch-bytes`.
pub const PREP_DEFAULT_BATCH_BYTES: usize = 16 * 1024 * 1024;
/// Default `--threads`.
pub const PREP_DEFAULT_THREADS: usize = 8;
/// Default `--max-run-bytes`: 256 MiB of expected new input per commit wave.
pub const PREP_DEFAULT_RUN_BYTES: u64 = 256 * 1024 * 1024;

impl PrepCommand {
    /// Whether the catalogue default root of `harness` belongs to this run.
    pub fn wants_default_root(&self, harness: &str) -> bool {
        !self.no_default_roots && self.selects(harness)
    }

    /// Whether `--harness` filters admit `harness` (no filter admits all).
    pub fn selects(&self, harness: &str) -> bool {
        self.harnesses.is_empty() || self.harnesses.iter().any(|value| value == harness)
    }

    /// Explicit `--root` sets in argv order.
    pub fn explicit_sets(&self) -> Vec<PrepSourceSet> {
        self.roots.iter().map(PrepRoot::source_set).collect()
    }

    /// The port request for shell-resolved `roots`.
    pub fn request(&self, roots: Vec<PrepSourceSet>) -> PrepRequest {
        PrepRequest {
            target: self.target.clone(),
            roots,
            options: self.options,
            limits: self.limits,
            threads: self.threads,
            max_run_bytes: self.max_run_bytes,
            modified_since_ns: self.modified_since_ns,
        }
    }
}

/// Parsed `unisphere prep compact`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepCompactCommand {
    pub target: PathBuf,
    pub mode: OutputMode,
}

impl PrepCompactCommand {
    pub fn request(&self) -> PrepCompactRequest {
        PrepCompactRequest {
            target: self.target.clone(),
        }
    }
}

/// Parsed `unisphere prep record`; the parser refuses it without `--include-content`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepRecordCommand {
    pub target: PathBuf,
    pub source: String,
    pub address: NativeAddress,
    pub include_content: bool,
    pub max_bytes: usize,
    pub mode: OutputMode,
}

impl PrepRecordCommand {
    pub fn request(&self) -> PrepRecordRequest {
        PrepRecordRequest {
            target: self.target.clone(),
            source: self.source.clone(),
            address: self.address.clone(),
            include_content: self.include_content,
            max_bytes: self.max_bytes,
        }
    }
}

/// Parsed `unisphere prep recipes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepRecipesCommand {
    pub mode: OutputMode,
}

/// Parsed `unisphere prep recipe NAME --target DIR`. `name` is resolved by
/// [`crate::run_prep_recipe`], which refuses unknown names with the valid list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepRecipeCommand {
    pub name: String,
    /// Absolute, lexically normalised target directory.
    pub target: PathBuf,
    /// Diagnostic format only; the script is always written raw.
    pub mode: OutputMode,
}

#[derive(Clone, PartialEq)]
pub enum ParsedCommand {
    Prep(PrepCommand),
    PrepCompact(PrepCompactCommand),
    PrepRecord(PrepRecordCommand),
    PrepRecipes(PrepRecipesCommand),
    PrepRecipe(PrepRecipeCommand),
    Config(ConfigCommand),
    Catalog(CatalogCommand),
    Docs(DocsCommand),
    Schema(SchemaCommand),
    Query(QueryCommand),
    PijQuery(PijQueryCommand),
    NativeRootList(NativeRootListCommand),
    NativeGitNotesList(NativeGitNotesListCommand),
    NativeExport(NativeExportCommand),
    Help(HelpCommand),
    Version { mode: OutputMode },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliParseFailure {
    code: &'static str,
    message: &'static str,
    next_action: &'static str,
    alternatives: Vec<String>,
    retryable: bool,
}

impl CliParseFailure {
    pub(crate) fn arguments() -> Self {
        Self {
            code: "UNI-CLI-ARGUMENT",
            message: "The command arguments do not match a supported Unisphere command.",
            next_action: "Run `unisphere --help` or `unisphere docs get start`, then choose one explicit scope and output mode.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    fn scope() -> Self {
        Self {
            code: "UNI-CLI-SCOPE",
            message: "A query requires exactly one of --pij, --repo, --source, or --input.",
            next_action: "Choose one explicit scope; use `--repo .` only when repository-associated local evidence is intended.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    fn pij_id() -> Self {
        Self {
            code: "UNI-CLI-PIJ-ID",
            message: "The Pij seat ID is not valid.",
            next_action: "Supply one explicit Pij seat ID without leading option syntax, whitespace, or control characters.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    fn pij_conflict() -> Self {
        Self {
            code: "UNI-CLI-PIJ-CONFLICT",
            message: "The Pij selector conflicts with an explicit native source or session selector.",
            next_action: "Use --pij ID as the only source/session identity; remove --repo, --source, --input, --repo-scope, --session, --native-id, --harness, --source-adapter, and excluded identity selectors.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    fn invalid_field(dataset: Dataset) -> Self {
        Self {
            code: "UNI-QUERY-FIELD",
            message: "This dataset does not declare the requested field.",
            next_action: "Choose a declared field from `unisphere schema show DATASET`.",
            alternatives: schema(dataset)
                .fields
                .iter()
                .map(|field| field.id.as_str().to_owned())
                .collect(),
            retryable: false,
        }
    }

    fn invalid_metric(dataset: Dataset) -> Self {
        Self {
            code: "UNI-QUERY-OPERATION",
            message: "This dataset does not declare the requested statistic.",
            next_action: "Choose a metric returned by `unisphere schema show DATASET`.",
            alternatives: schema(dataset)
                .metrics
                .iter()
                .map(|metric| metric.as_str().to_owned())
                .collect(),
            retryable: false,
        }
    }

    fn choices(
        code: &'static str,
        message: &'static str,
        next_action: &'static str,
        alternatives: &[&str],
    ) -> Self {
        Self {
            code,
            message,
            next_action,
            alternatives: alternatives
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            retryable: false,
        }
    }

    fn mixed_native_query() -> Self {
        Self {
            code: "UNI-CLI-ROUTE",
            message: "Native session selectors cannot be mixed with query selectors.",
            next_action: "Use `sessions list --root PATH`, `sessions list --adapter git-ai --repo PATH`, or a query form with --source-adapter; do not combine them.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    fn from_query(failure: QueryFailure) -> Self {
        let alternatives = match failure.recovery() {
            RecoveryAction::ChooseField { allowed, .. } => allowed
                .iter()
                .map(|value| value.as_str().to_owned())
                .collect(),
            RecoveryAction::ChooseAdapter { allowed } => allowed
                .iter()
                .map(|value| value.as_str().to_owned())
                .collect(),
            RecoveryAction::ChooseEntity { candidates, .. }
            | RecoveryAction::ChooseBranch { candidates } => {
                candidates.iter().map(ToString::to_string).collect()
            }
            RecoveryAction::ConsultSchema { dataset } => schema(*dataset)
                .fields
                .iter()
                .map(|field| field.id.as_str().to_owned())
                .collect(),
            _ => Vec::new(),
        };
        Self {
            code: failure.code(),
            message: failure.message(),
            next_action: failure.recovery().guidance(),
            alternatives,
            retryable: failure.retryable(),
        }
    }

    fn dataset() -> Self {
        Self {
            code: "UNI-CLI-DATASET",
            message: "The requested query dataset is not supported.",
            next_action: "Choose one dataset returned by `unisphere --help` and inspect it with `unisphere schema show DATASET`.",
            alternatives: Dataset::ALL
                .iter()
                .map(|dataset| dataset.as_str().to_owned())
                .collect(),
            retryable: false,
        }
    }

    fn prep_content() -> Self {
        Self {
            code: "UNI-CLI-PREP-CONTENT",
            message: "`prep record` emits one native record, which is content; it requires explicit --include-content.",
            next_action: "Re-run the same `unisphere prep record` command with --include-content only if emitting that native record is intended; prepped tables stay metadata-only either way.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    fn prep_root() -> Self {
        Self {
            code: "UNI-CLI-PREP-ROOT",
            message: "An explicit prep root is malformed, reuses a harness/label or harness/directory, or uses the reserved label `default`.",
            next_action: "Write each root once as --root HARNESS=DIR or --root HARNESS:LABEL=DIR; HARNESS and LABEL use lowercase letters, digits, '.', '_' or '-', and `default` names only the catalogue root.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    fn prep_scope() -> Self {
        Self {
            code: "UNI-CLI-PREP-SCOPE",
            message: "The prep root selection is empty or contradicts --harness.",
            next_action: "Give at least one --root with --no-default-roots, and list the harness of every explicit --root in --harness when --harness is used.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    fn prep_limits() -> Self {
        Self {
            code: "UNI-CLI-PREP-LIMITS",
            message: "Prep limits (including --threads and --max-run-bytes) must be positive, --max-batch-bytes at least --max-record-bytes, and --modified-since an RFC 3339 instant; --max-snapshot-bytes and --max-snapshot-records must be positive.",
            next_action: "Choose --threads N >= 1, --max-run-bytes N >= 1, --max-record-bytes N >= 1, --max-batch-bytes N >= --max-record-bytes, --max-snapshot-bytes N >= 1, --max-snapshot-records N >= 1, and --modified-since like 2026-01-31T00:00:00Z.",
            alternatives: Vec::new(),
            retryable: false,
        }
    }

    pub const fn code(&self) -> &'static str {
        self.code
    }
    pub const fn message(&self) -> &'static str {
        self.message
    }
    pub const fn next_action(&self) -> &'static str {
        self.next_action
    }
    pub fn alternatives(&self) -> &[String] {
        &self.alternatives
    }
    pub const fn retryable(&self) -> bool {
        self.retryable
    }
}

/// Parse one complete invocation into a typed command without opening configuration,
/// source stores, Git, the environment, or output destinations.
pub fn parse(args: Vec<OsString>, context: &CliContext) -> Result<ParsedCommand, CliParseFailure> {
    let selected_mode = diagnostic_mode(&args, context.stdout_is_terminal);
    let matches = match Cli::command().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            return Ok(ParsedCommand::Help(HelpCommand {
                text: error.to_string(),
                mode: selected_mode,
            }));
        }
        Err(_) => return Err(CliParseFailure::arguments()),
    };
    let cli = Cli::from_arg_matches(&matches).map_err(|_| CliParseFailure::arguments())?;
    let selected_mode = if cli.human {
        OutputMode::Human
    } else if cli.json {
        OutputMode::Json
    } else {
        selected_mode
    };
    if cli.version {
        return Ok(ParsedCommand::Version {
            mode: selected_mode,
        });
    }
    match cli.command.ok_or_else(CliParseFailure::arguments)? {
        RootCommand::Prep(args) => prep_command(args, context, selected_mode),
        RootCommand::Config {
            command: ConfigSubcommand::Check(check),
        } => Ok(ParsedCommand::Config(ConfigCommand {
            config: check.config,
            source_roots: check.source_roots,
            clear_source_roots: check.clear_source_roots,
            mode: selected_mode,
        })),
        RootCommand::Adapters {
            command: AdapterSubcommand::List,
        } => Ok(ParsedCommand::Catalog(CatalogCommand {
            mode: selected_mode,
        })),
        RootCommand::Docs { command } => Ok(ParsedCommand::Docs(match command {
            DocsSubcommand::List => DocsCommand::List {
                mode: selected_mode,
            },
            DocsSubcommand::Get { topic } => DocsCommand::Get {
                topic,
                mode: selected_mode,
            },
        })),
        RootCommand::Schema {
            command: SchemaSubcommand::Show { dataset },
        } => Ok(ParsedCommand::Schema(SchemaCommand {
            dataset: dataset.parse().map_err(|_| CliParseFailure::dataset())?,
            mode: selected_mode,
        })),
        RootCommand::Sources { command } => match command {
            SourcesSubcommand::List(query) => query_command(
                Dataset::Sources,
                Operation::List,
                query,
                context,
                selected_mode,
            ),
            SourcesSubcommand::Check(query) => {
                if query.pij.is_some() {
                    pij_target(
                        query_command(
                            Dataset::Sources,
                            Operation::List,
                            query,
                            context,
                            selected_mode,
                        ),
                        PijTarget::SourceCheck,
                    )
                } else {
                    let source = query
                        .source
                        .as_ref()
                        .and_then(|source| source.to_str())
                        .and_then(|source| SourceId::from_str(source).ok())
                        .ok_or_else(CliParseFailure::arguments)?;
                    query_command(
                        Dataset::Sources,
                        Operation::Check { source },
                        query,
                        context,
                        selected_mode,
                    )
                }
            }
        },
        RootCommand::Sessions { command } => match command {
            SessionsSubcommand::List(args) => session_list(args, context, selected_mode),
            SessionsSubcommand::Show(args) => session_show_command(args, context, selected_mode),
            SessionsSubcommand::Tree(args) => {
                let session = parse_entity(&args.entity)?;
                query_command(
                    Dataset::Sessions,
                    Operation::Tree { session },
                    args.query,
                    context,
                    selected_mode,
                )
            }
            SessionsSubcommand::Stats(args) => {
                stats_command(Dataset::Sessions, args, context, selected_mode)
            }
            SessionsSubcommand::Extract(query) => query_command(
                Dataset::Sessions,
                Operation::Extract,
                query,
                context,
                selected_mode,
            ),
            SessionsSubcommand::Export(args) => native_export(args, context),
        },
        RootCommand::Turns { command } => query_dataset(
            Dataset::Turns,
            match command {
                TurnsSubcommand::List(query) => QueryLeaf::Simple(Operation::List, query),
                TurnsSubcommand::Show(show) => QueryLeaf::Show(show),
                TurnsSubcommand::Stats(stats) => QueryLeaf::Stats(stats),
                TurnsSubcommand::Extract(query) => QueryLeaf::Simple(Operation::Extract, query),
            },
            context,
            selected_mode,
        ),
        RootCommand::Messages { command } => query_dataset(
            Dataset::Messages,
            match command {
                MessagesSubcommand::List(query) => QueryLeaf::Simple(Operation::List, query),
                MessagesSubcommand::Show(show) => QueryLeaf::Show(show),
                MessagesSubcommand::Extract(query) => QueryLeaf::Simple(Operation::Extract, query),
            },
            context,
            selected_mode,
        ),
        RootCommand::Tools { command } => query_dataset(
            Dataset::Tools,
            match command {
                ToolsSubcommand::List(query) => QueryLeaf::Simple(Operation::List, query),
                ToolsSubcommand::Show(show) => QueryLeaf::Show(show),
                ToolsSubcommand::Stats(stats) => QueryLeaf::Stats(stats),
                ToolsSubcommand::Extract(query) => QueryLeaf::Simple(Operation::Extract, query),
            },
            context,
            selected_mode,
        ),
        RootCommand::Events { command } => query_dataset(
            Dataset::Events,
            match command {
                EventsSubcommand::List(query) => QueryLeaf::Simple(Operation::List, query),
                EventsSubcommand::Show(show) => QueryLeaf::Show(show),
                EventsSubcommand::Extract(query) => QueryLeaf::Simple(Operation::Extract, query),
            },
            context,
            selected_mode,
        ),
    }
}

fn prep_command(
    args: PrepArgs,
    context: &CliContext,
    mode: OutputMode,
) -> Result<ParsedCommand, CliParseFailure> {
    match args.command {
        Some(PrepSubcommand::Compact { target }) => {
            return Ok(ParsedCommand::PrepCompact(PrepCompactCommand {
                target: prep_path(target, context)?,
                mode,
            }));
        }
        Some(PrepSubcommand::Record {
            target,
            source,
            offset,
            key,
            include_content,
        }) => {
            if !include_content {
                return Err(CliParseFailure::prep_content());
            }
            if source.is_empty() || key.as_ref().is_some_and(String::is_empty) {
                return Err(CliParseFailure::arguments());
            }
            return Ok(ParsedCommand::PrepRecord(PrepRecordCommand {
                target: prep_path(target, context)?,
                source,
                address: NativeAddress { offset, key },
                include_content,
                max_bytes: PREP_DEFAULT_BATCH_BYTES,
                mode,
            }));
        }
        Some(PrepSubcommand::Recipes) => {
            return Ok(ParsedCommand::PrepRecipes(PrepRecipesCommand { mode }));
        }
        Some(PrepSubcommand::Recipe { name, target }) => {
            return Ok(ParsedCommand::PrepRecipe(PrepRecipeCommand {
                name,
                target: prep_path(target, context)?,
                mode,
            }));
        }
        None => {}
    }
    let target = prep_path(args.target.ok_or_else(CliParseFailure::arguments)?, context)?;
    let mut harnesses = Vec::with_capacity(args.harnesses.len());
    for harness in args.harnesses {
        if !prep_name(&harness) {
            return Err(CliParseFailure::prep_scope());
        }
        harnesses.push(harness);
    }
    harnesses.sort_unstable();
    harnesses.dedup();
    let mut roots: Vec<PrepRoot> = Vec::with_capacity(args.roots.len());
    for value in &args.roots {
        let root = prep_root(value, context)?;
        if !harnesses.is_empty() && !harnesses.contains(&root.harness) {
            return Err(CliParseFailure::prep_scope());
        }
        let set = root.source_set();
        if roots.iter().any(|seen| {
            seen.harness == root.harness
                && (seen.source_set().label == set.label || seen.root == root.root)
        }) {
            return Err(CliParseFailure::prep_root());
        }
        roots.push(root);
    }
    if args.no_default_roots && roots.is_empty() {
        return Err(CliParseFailure::prep_scope());
    }
    let max_batch_bytes = args.max_batch_bytes.unwrap_or(PREP_DEFAULT_BATCH_BYTES);
    let read = ReadLimits {
        max_records: usize::MAX,
        // Real Oh My Pi and Copilot CLI records reach ~13 MiB (inlined tool
        // output): prep accepts a record up to the batch bound, which alone
        // bounds memory per source.
        max_record_bytes: args.max_record_bytes.unwrap_or(max_batch_bytes),
        max_batch_bytes,
    };
    let threads = args.threads.unwrap_or(PREP_DEFAULT_THREADS);
    let max_run_bytes = args.max_run_bytes.unwrap_or(PREP_DEFAULT_RUN_BYTES);
    let default_snapshot = SnapshotLimits::default();
    let max_snapshot_bytes = args
        .max_snapshot_bytes
        .unwrap_or(default_snapshot.max_snapshot_bytes);
    let snapshot = SnapshotLimits {
        max_records: args
            .max_snapshot_records
            .unwrap_or(default_snapshot.max_records),
        // One record never exceeds the snapshot holding it.
        max_record_bytes: default_snapshot.max_record_bytes.min(max_snapshot_bytes),
        max_snapshot_bytes,
    };
    if threads == 0
        || max_run_bytes == 0
        || read.validate().is_err()
        || snapshot.validate().is_err()
    {
        return Err(CliParseFailure::prep_limits());
    }
    let modified_since_ns = args
        .modified_since
        .as_deref()
        .map(|value| Timestamp::parse(value, TimestampBasis::SuppliedUnknown))
        .transpose()
        .map_err(|_| CliParseFailure::prep_limits())?
        .map(|timestamp| timestamp.unix_nanos());
    Ok(ParsedCommand::Prep(PrepCommand {
        target,
        roots,
        harnesses,
        no_default_roots: args.no_default_roots,
        options: PrepOptions {
            include_content: args.include_content,
        },
        limits: PrepReadLimits { read, snapshot },
        threads,
        max_run_bytes,
        modified_since_ns,
        mode,
    }))
}

/// `HARNESS[:LABEL]=DIR`; DIR is made absolute and lexically normalised so the
/// derived label (and every source key) is stable across spellings.
fn prep_root(value: &str, context: &CliContext) -> Result<PrepRoot, CliParseFailure> {
    let (name, dir) = value
        .split_once('=')
        .ok_or_else(CliParseFailure::prep_root)?;
    let (harness, label) = match name.split_once(':') {
        Some((harness, label)) => (harness, Some(label)),
        None => (name, None),
    };
    if !prep_name(harness)
        || label.is_some_and(|label| !prep_name(label) || label == DEFAULT_ROOT_LABEL)
        || dir.is_empty()
    {
        return Err(CliParseFailure::prep_root());
    }
    Ok(PrepRoot {
        harness: harness.to_owned(),
        label: label.map(str::to_owned),
        root: prep_path(PathBuf::from(dir), context)?,
    })
}

/// Harness ids and labels are source-key segments: `[a-z0-9._-]+`, not starting
/// with `.` or `-`.
fn prep_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with(['.', '-'])
        && value
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

fn prep_path(path: PathBuf, context: &CliContext) -> Result<PathBuf, CliParseFailure> {
    Ok(absolute(path, context)?.components().collect())
}

enum QueryLeaf {
    Simple(Operation, QueryArgs),
    Show(ShowArgs),
    Stats(StatsArgs),
}

fn query_dataset(
    dataset: Dataset,
    leaf: QueryLeaf,
    context: &CliContext,
    mode: OutputMode,
) -> Result<ParsedCommand, CliParseFailure> {
    match leaf {
        QueryLeaf::Simple(operation, query) => {
            query_command(dataset, operation, query, context, mode)
        }
        QueryLeaf::Show(show) => show_command(dataset, show, context, mode),
        QueryLeaf::Stats(stats) => stats_command(dataset, stats, context, mode),
    }
}

fn show_command(
    dataset: Dataset,
    args: ShowArgs,
    context: &CliContext,
    mode: OutputMode,
) -> Result<ParsedCommand, CliParseFailure> {
    query_command(
        dataset,
        Operation::Show {
            entity: parse_entity(&args.entity)?,
        },
        args.query,
        context,
        mode,
    )
}

fn session_show_command(
    args: SessionShowArgs,
    context: &CliContext,
    mode: OutputMode,
) -> Result<ParsedCommand, CliParseFailure> {
    match (args.entity, args.query.pij.is_some()) {
        (None, true) => pij_target(
            query_command(
                Dataset::Sessions,
                Operation::List,
                args.query,
                context,
                mode,
            ),
            PijTarget::SessionShow,
        ),
        (Some(_), true) => Err(CliParseFailure::pij_conflict()),
        (Some(entity), false) => query_command(
            Dataset::Sessions,
            Operation::Show {
                entity: parse_entity(&entity)?,
            },
            args.query,
            context,
            mode,
        ),
        (None, false) => Err(CliParseFailure::arguments()),
    }
}

fn pij_target(
    command: Result<ParsedCommand, CliParseFailure>,
    target: PijTarget,
) -> Result<ParsedCommand, CliParseFailure> {
    match command? {
        ParsedCommand::PijQuery(mut command) => {
            command.target = target;
            Ok(ParsedCommand::PijQuery(command))
        }
        _ => Err(CliParseFailure::arguments()),
    }
}

fn stats_command(
    dataset: Dataset,
    args: StatsArgs,
    context: &CliContext,
    mode: OutputMode,
) -> Result<ParsedCommand, CliParseFailure> {
    let group_by = args
        .group_by
        .iter()
        .map(|value| parse_field(dataset, value))
        .collect::<Result<Vec<_>, _>>()?;
    let metrics = if args.metrics.is_empty() {
        schema(dataset).metrics.to_vec()
    } else {
        args.metrics
            .iter()
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| CliParseFailure::invalid_metric(dataset))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    query_command(
        dataset,
        Operation::Stats { group_by, metrics },
        args.query,
        context,
        mode,
    )
}

fn session_list(
    args: SessionListArgs,
    context: &CliContext,
    mode: OutputMode,
) -> Result<ParsedCommand, CliParseFailure> {
    let has_git_options = args.has_git_options();
    let invalid_git_limits = args.git_limits_invalid();
    let empty_notes_ref = args.notes_ref.as_deref() == Some("");
    let invalid_git_executable = args
        .git_executable
        .as_ref()
        .is_some_and(|path| !path.is_absolute());
    if let Some(root) = args.root {
        if args.adapter.is_some() || args.query.has_query_selector() || has_git_options {
            return Err(CliParseFailure::mixed_native_query());
        }
        if args.max_sessions == Some(0) {
            return Err(CliParseFailure::arguments());
        }
        return Ok(ParsedCommand::NativeRootList(NativeRootListCommand {
            root: absolute(root, context)?,
            max_sessions: args.max_sessions.unwrap_or(4_096),
        }));
    }
    if let Some(adapter) = args.adapter {
        if args.max_sessions.is_some() || args.query.has_query_option_besides_repo() {
            return Err(CliParseFailure::mixed_native_query());
        }
        if adapter != "git-ai" || invalid_git_limits || empty_notes_ref || invalid_git_executable {
            return Err(CliParseFailure::arguments());
        }
        let repo = args.query.repo.ok_or_else(CliParseFailure::arguments)?;
        return Ok(ParsedCommand::NativeGitNotesList(
            NativeGitNotesListCommand {
                adapter,
                repo: absolute(repo, context)?,
                notes_ref: args.notes_ref.unwrap_or_else(|| "refs/notes/ai".to_owned()),
                commits: args.commit,
                git_executable: args.git_executable,
                max_notes: args.max_notes.unwrap_or(1_000),
                max_records: args.max_records.unwrap_or(10_000),
                max_note_bytes: args.max_note_bytes.unwrap_or(1_048_576),
                max_total_bytes: args.max_total_bytes.unwrap_or(16_777_216),
                max_listing_bytes: args.max_listing_bytes.unwrap_or(1_048_576),
                command_timeout_ms: args.command_timeout_ms.unwrap_or(5_000),
            },
        ));
    }
    if args.max_sessions.is_some() || has_git_options {
        return Err(CliParseFailure::mixed_native_query());
    }
    query_command(
        Dataset::Sessions,
        Operation::List,
        args.query,
        context,
        mode,
    )
}

fn native_export(
    args: NativeExportArgs,
    context: &CliContext,
) -> Result<ParsedCommand, CliParseFailure> {
    let has_git_options = args.has_git_options();
    let invalid_git_limits = args.git_limits_invalid();
    let empty_notes_ref = args.notes_ref.as_deref() == Some("");
    let invalid_git_executable = args
        .git_executable
        .as_ref()
        .is_some_and(|path| !path.is_absolute());
    if args.input.is_some() == args.repo.is_some()
        || args.max_records == Some(0)
        || args.max_record_bytes == Some(0)
        || args.max_batch_bytes == Some(0)
        || args.max_snapshot_bytes == Some(0)
    {
        return Err(CliParseFailure::arguments());
    }
    let adapter = validate_registry_name(args.adapter)?;
    let git_notes = adapter == "git-ai";
    if git_notes != args.repo.is_some() {
        return Err(CliParseFailure::arguments());
    }
    if git_notes {
        if invalid_git_limits
            || empty_notes_ref
            || invalid_git_executable
            || args.source_format.is_some()
            || args.table.is_some()
            || args.session_id.is_some()
            || args.max_record_bytes.is_some()
            || args.max_batch_bytes.is_some()
            || args.max_snapshot_bytes.is_some()
        {
            return Err(CliParseFailure::arguments());
        }
    } else {
        if has_git_options {
            return Err(CliParseFailure::arguments());
        }
        if args.source_format.as_deref().is_some_and(|format| {
            !matches!(
                format,
                "json-document" | "json-journal" | "sqlite-key-value"
            )
        }) {
            return Err(CliParseFailure::arguments());
        }
    }
    Ok(ParsedCommand::NativeExport(NativeExportCommand {
        adapter,
        input: args.input.map(|path| absolute(path, context)).transpose()?,
        repo: args.repo.map(|path| absolute(path, context)).transpose()?,
        output: args
            .output
            .map(|path| absolute(path, context))
            .transpose()?,
        include_content: args.include_content,
        source_format: args.source_format,
        table: args.table,
        session_id: args.session_id,
        max_records: if git_notes {
            Some(args.max_records.unwrap_or(10_000))
        } else {
            args.max_records
        },
        max_record_bytes: args.max_record_bytes,
        max_batch_bytes: args.max_batch_bytes,
        max_snapshot_bytes: args.max_snapshot_bytes,
        notes_ref: git_notes.then(|| args.notes_ref.unwrap_or_else(|| "refs/notes/ai".to_owned())),
        commits: args.commit,
        git_executable: args.git_executable,
        max_notes: git_notes.then(|| args.max_notes.unwrap_or(1_000)),
        max_note_bytes: git_notes.then(|| args.max_note_bytes.unwrap_or(1_048_576)),
        max_total_bytes: git_notes.then(|| args.max_total_bytes.unwrap_or(16_777_216)),
        max_listing_bytes: git_notes.then(|| args.max_listing_bytes.unwrap_or(1_048_576)),
        command_timeout_ms: git_notes.then(|| args.command_timeout_ms.unwrap_or(5_000)),
    }))
}

fn query_command(
    dataset: Dataset,
    operation: Operation,
    args: QueryArgs,
    context: &CliContext,
    mode: OutputMode,
) -> Result<ParsedCommand, CliParseFailure> {
    let pij_id = args.pij.as_deref().map(validate_pij_id).transpose()?;
    let scope = query_scope(&args, context)?;
    let stdin = matches!(
        &scope,
        QueryScope::Offline {
            input: OfflineRef::Stdin
        }
    );
    if args.stdin_format.is_some() && !stdin {
        return Err(CliParseFailure::arguments());
    }
    let stdin_format = match args.stdin_format.as_deref().unwrap_or("json") {
        "json" => SavedFormat::QueryJsonV1,
        "jsonl" => SavedFormat::QueryJsonlV1,
        _ => {
            return Err(CliParseFailure::choices(
                "UNI-CLI-ARGUMENT",
                "The stdin framing format is not supported.",
                "Choose json or jsonl; omit --stdin-format for JSON.",
                &["json", "jsonl"],
            ));
        }
    };
    let mut filters = Vec::new();
    strings_filter(
        &mut filters,
        FieldId::Harness,
        &args.harness,
        args.ignore_case,
    )?;
    strings_filter(
        &mut filters,
        FieldId::Adapter,
        &args.source_adapter,
        args.ignore_case,
    )?;
    strings_filter(
        &mut filters,
        FieldId::NativeId,
        &args.native_id,
        args.ignore_case,
    )?;
    string_predicate_filter(
        &mut filters,
        FieldId::Name,
        Predicate::Glob,
        &args.name,
        args.ignore_case,
    )?;
    strings_filter(&mut filters, FieldId::Role, &args.role, args.ignore_case)?;
    strings_filter(
        &mut filters,
        FieldId::Roles,
        &args.has_role,
        args.ignore_case,
    )?;
    strings_filter(
        &mut filters,
        FieldId::ToolNames,
        &args.has_tool,
        args.ignore_case,
    )?;
    strings_filter(
        &mut filters,
        FieldId::ToolFamilies,
        &args.has_tool_family,
        args.ignore_case,
    )?;
    strings_filter(
        &mut filters,
        FieldId::ToolName,
        &args.tool,
        args.ignore_case,
    )?;
    strings_filter(
        &mut filters,
        FieldId::ToolFamily,
        &args.tool_family,
        args.ignore_case,
    )?;
    strings_filter(
        &mut filters,
        FieldId::Status,
        &args.status,
        args.ignore_case,
    )?;
    strings_filter(&mut filters, FieldId::Kind, &args.kind, args.ignore_case)?;
    exclusion_filter(&mut filters, FieldId::Harness, &args.exclude_harness)?;
    exclusion_filter(&mut filters, FieldId::Adapter, &args.exclude_source_adapter)?;

    let session_field = if dataset == Dataset::Sessions {
        FieldId::Id
    } else {
        FieldId::SessionId
    };
    entity_filter(&mut filters, session_field, &args.session)?;
    entity_filter(&mut filters, FieldId::ParentIds, &args.parent)?;
    entity_filter(&mut filters, FieldId::TurnId, &args.turn)?;
    entity_filter(&mut filters, FieldId::CallId, &args.call)?;

    let model_field = if dataset == Dataset::Sessions {
        FieldId::Models
    } else {
        FieldId::Model
    };
    strings_filter(&mut filters, model_field, &args.model, args.ignore_case)?;

    if !args.contains.is_empty() || !args.regex.is_empty() {
        let field = match dataset {
            Dataset::Sessions => FieldId::Name,
            Dataset::Messages => FieldId::Text,
            Dataset::Tools => FieldId::Command,
            _ => return Err(CliParseFailure::arguments()),
        };
        string_predicate_filter(
            &mut filters,
            field,
            Predicate::Contains,
            &args.contains,
            args.ignore_case,
        )?;
        string_predicate_filter(
            &mut filters,
            field,
            Predicate::Regex,
            &args.regex,
            args.ignore_case,
        )?;
    }
    string_predicate_filter(
        &mut filters,
        FieldId::Command,
        Predicate::Contains,
        &args.command_contains,
        args.ignore_case,
    )?;
    unsigned_filter(&mut filters, FieldId::TurnCount, args.min_turns)?;
    unsigned_filter(&mut filters, FieldId::ToolCallCount, args.min_tool_calls)?;
    duration_filter(&mut filters, args.min_duration)?;
    if args.has_errors {
        filters.push(Filter {
            field: FieldId::HasErrors,
            predicate: Predicate::Equal,
            values: vec![FieldValue::Bool(true)],
            ignore_case: false,
        });
    }
    if args.has_duration {
        filters.push(Filter {
            field: FieldId::DurationMs,
            predicate: Predicate::Has,
            values: Vec::new(),
            ignore_case: false,
        });
    }
    if !args.exit_code.is_empty() {
        filters.push(Filter {
            field: FieldId::ExitCode,
            predicate: Predicate::In,
            values: args
                .exit_code
                .into_iter()
                .map(FieldValue::Integer)
                .collect(),
            ignore_case: false,
        });
    }

    let time = TimeWindow {
        field: args
            .time_field
            .as_deref()
            .map(|value| parse_field(dataset, value))
            .transpose()?,
        since: args
            .since
            .as_deref()
            .map(|value| Timestamp::parse(value, TimestampBasis::SuppliedUnknown))
            .transpose()
            .map_err(CliParseFailure::from_query)?,
        until: args
            .until
            .as_deref()
            .map(|value| Timestamp::parse(value, TimestampBasis::SuppliedUnknown))
            .transpose()
            .map_err(CliParseFailure::from_query)?,
        include_undated: args.include_undated,
    };
    let branch = args.branch.as_deref().map(parse_entity).transpose()?;
    let turn_range = args.range.as_deref().map(parse_range).transpose()?;
    let sort = args
        .sort
        .iter()
        .map(|value| {
            let (direction, field) = value
                .strip_prefix('-')
                .map_or((SortDirection::Ascending, value.as_str()), |field| {
                    (SortDirection::Descending, field)
                });
            Ok(SortKey {
                field: parse_field(dataset, field)?,
                direction,
            })
        })
        .collect::<Result<Vec<_>, CliParseFailure>>()?;
    let mut columns = if args.columns.is_empty() {
        None
    } else {
        Some(
            args.columns
                .iter()
                .map(|value| parse_field(dataset, value))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    if let Some(part) = args.part.as_deref() {
        if dataset != Dataset::Tools
            || operation.kind() != unisphere_core::query::OperationKind::Extract
        {
            return Err(CliParseFailure::arguments());
        }
        let selected = match part {
            "input" => &[FieldId::Input][..],
            "output" => &[FieldId::Output][..],
            "all" => &[FieldId::Input, FieldId::Output][..],
            _ => return Err(CliParseFailure::arguments()),
        };
        let projected = columns.get_or_insert_with(Vec::new);
        for field in selected {
            if !projected.contains(field) {
                projected.push(*field);
            }
        }
    }
    let mut limits = QueryLimits::default();
    if let Some(max_output_bytes) = args.max_output_bytes {
        limits.max_output_bytes = max_output_bytes;
    }
    let request = QueryRequest {
        dataset,
        operation,
        scope,
        filters,
        time,
        branch,
        turn_range,
        sort,
        columns,
        limit: args.limit,
        cursor: args.cursor,
        context: ContextWindow {
            before: args.context_before,
            after: args.context_after,
        },
        include_content: args.include_content,
        allow_partial: args.allow_partial,
        unresolved: match args.unresolved.as_str() {
            "reject" => unisphere_core::query::UnresolvedPolicy::Reject,
            "separate" => unisphere_core::query::UnresolvedPolicy::Separate,
            _ => {
                return Err(CliParseFailure::choices(
                    "UNI-CLI-ARGUMENT",
                    "The unresolved-identity policy is not supported.",
                    "Choose reject or separate.",
                    &["reject", "separate"],
                ));
            }
        },
        limits,
    };
    if pij_id.is_some() && request.turn_range.is_some() {
        let mut validation_request = request.clone();
        validation_request.turn_range = None;
        validation_request
            .validate()
            .map_err(CliParseFailure::from_query)?;
    } else {
        request.validate().map_err(CliParseFailure::from_query)?;
    }

    let format = match args.format.as_deref() {
        Some(value) => value.parse().map_err(|_| {
            CliParseFailure::choices(
                "UNI-CLI-ARGUMENT",
                "The requested output format is not supported.",
                "Choose a format declared by `unisphere schema show DATASET`.",
                &["json", "jsonl", "csv", "table", "text", "markdown"],
            )
        })?,
        None => match mode {
            OutputMode::Json | OutputMode::JsonDiagnostic => OutputFormat::Json,
            OutputMode::Human => OutputFormat::Table,
        },
    };
    if schema(dataset)
        .format(request.operation.kind(), format)
        .is_none()
    {
        return Err(CliParseFailure::from_query(QueryFailure::new(
            unisphere_core::query::QueryFailureCode::UnsupportedOperation,
            unisphere_core::query::RecoveryAction::ConsultSchema { dataset },
        )));
    }
    let csv_safety = match args.csv_safety.as_deref() {
        None => CsvSafety::Spreadsheet,
        Some(value) if format == OutputFormat::Csv => value.parse().map_err(|_| {
            CliParseFailure::choices(
                "UNI-CLI-ARGUMENT",
                "The CSV safety mode is not supported.",
                "Choose spreadsheet or raw.",
                &["spreadsheet", "raw"],
            )
        })?,
        Some(_) => return Err(CliParseFailure::arguments()),
    };
    let query = QueryCommand {
        request,
        format,
        csv_safety,
        output: args
            .output
            .map(|path| absolute(path, context))
            .transpose()?,
        diagnostic_mode: mode,
        stdin_format,
    };
    Ok(match pij_id {
        Some(id) => ParsedCommand::PijQuery(PijQueryCommand {
            id,
            query,
            target: PijTarget::Query,
        }),
        None => ParsedCommand::Query(query),
    })
}

fn query_scope(args: &QueryArgs, context: &CliContext) -> Result<QueryScope, CliParseFailure> {
    if args.pij.is_some() && (args.repo.is_some() || args.source.is_some() || args.input.is_some())
    {
        return Err(CliParseFailure::pij_conflict());
    }
    let count = usize::from(args.pij.is_some())
        + usize::from(args.repo.is_some())
        + usize::from(args.source.is_some())
        + usize::from(args.input.is_some());
    if count != 1 {
        return Err(CliParseFailure::scope());
    }
    if args.pij.is_some() {
        if args.repo_scope != "tree"
            || !args.session.is_empty()
            || !args.native_id.is_empty()
            || !args.harness.is_empty()
            || !args.exclude_harness.is_empty()
            || !args.source_adapter.is_empty()
            || !args.exclude_source_adapter.is_empty()
        {
            return Err(CliParseFailure::pij_conflict());
        }
        return Ok(QueryScope::Repository {
            path: absolute(context.cwd.clone(), context)?,
            scope: "tree".parse().map_err(|_| CliParseFailure::arguments())?,
        });
    }
    if args.repo.is_none() && args.repo_scope != "tree" {
        return Err(CliParseFailure::arguments());
    }
    if let Some(repo) = &args.repo {
        return Ok(QueryScope::Repository {
            path: absolute(repo.clone(), context)?,
            scope: args.repo_scope.parse().map_err(|_| {
                CliParseFailure::choices(
                    "UNI-CLI-ARGUMENT",
                    "The repository scope is not supported.",
                    "Choose exact, tree, or worktrees.",
                    &["exact", "tree", "worktrees"],
                )
            })?,
        });
    }
    if let Some(source) = &args.source {
        let selector = source
            .to_str()
            .and_then(|value| SourceId::from_str(value).ok())
            .map_or_else(
                || {
                    Ok(SourceSelector::Path {
                        path: absolute(source.clone(), context)?,
                        adapter: single_source_adapter(args)?,
                    })
                },
                |id| Ok(SourceSelector::Id(id)),
            )?;
        return Ok(QueryScope::Source { selector });
    }
    let input = args.input.as_ref().expect("one scope was selected");
    Ok(QueryScope::Offline {
        input: if input == Path::new("-") {
            OfflineRef::Stdin
        } else {
            OfflineRef::File(absolute(input.clone(), context)?)
        },
    })
}

fn single_source_adapter(args: &QueryArgs) -> Result<Option<AdapterId>, CliParseFailure> {
    if args.source_adapter.len() == 1 {
        Ok(Some(parse_name(&args.source_adapter[0])?))
    } else {
        Ok(None)
    }
}

fn parse_range(value: &str) -> Result<InclusiveRange, CliParseFailure> {
    let (start, end) = value
        .split_once(':')
        .ok_or_else(CliParseFailure::arguments)?;
    InclusiveRange::new(
        start.parse().map_err(|_| CliParseFailure::arguments())?,
        end.parse().map_err(|_| CliParseFailure::arguments())?,
    )
    .map_err(CliParseFailure::from_query)
}

fn strings_filter(
    filters: &mut Vec<Filter>,
    field: FieldId,
    values: &[String],
    ignore_case: bool,
) -> Result<(), CliParseFailure> {
    string_predicate_filter(filters, field, Predicate::In, values, ignore_case)
}

fn exclusion_filter(
    filters: &mut Vec<Filter>,
    field: FieldId,
    values: &[String],
) -> Result<(), CliParseFailure> {
    string_predicate_filter(filters, field, Predicate::Exclude, values, false)
}

fn string_predicate_filter(
    filters: &mut Vec<Filter>,
    field: FieldId,
    predicate: Predicate,
    values: &[String],
    ignore_case: bool,
) -> Result<(), CliParseFailure> {
    if values.is_empty() {
        return Ok(());
    }
    if matches!(field, FieldId::Adapter) {
        for value in values {
            let _: AdapterId = parse_name(value)?;
        }
    }
    if matches!(field, FieldId::Harness) {
        for value in values {
            let _: HarnessId = parse_name(value)?;
        }
    }
    filters.push(Filter {
        field,
        predicate,
        values: values.iter().cloned().map(FieldValue::String).collect(),
        ignore_case,
    });
    Ok(())
}

fn entity_filter(
    filters: &mut Vec<Filter>,
    field: FieldId,
    values: &[String],
) -> Result<(), CliParseFailure> {
    if values.is_empty() {
        return Ok(());
    }
    filters.push(Filter {
        field,
        predicate: Predicate::In,
        values: values
            .iter()
            .map(|value| parse_entity(value).map(FieldValue::Id))
            .collect::<Result<Vec<_>, _>>()?,
        ignore_case: false,
    });
    Ok(())
}

fn parse_field(dataset: Dataset, value: &str) -> Result<FieldId, CliParseFailure> {
    value
        .parse()
        .map_err(|_| CliParseFailure::invalid_field(dataset))
}

fn unsigned_filter(
    filters: &mut Vec<Filter>,
    field: FieldId,
    value: Option<u64>,
) -> Result<(), CliParseFailure> {
    if let Some(value) = value {
        filters.push(Filter {
            field,
            predicate: Predicate::AtLeast,
            values: vec![FieldValue::Unsigned(value)],
            ignore_case: false,
        });
    }
    Ok(())
}

fn duration_filter(filters: &mut Vec<Filter>, value: Option<f64>) -> Result<(), CliParseFailure> {
    if let Some(value) = value {
        if !value.is_finite() || value < 0.0 {
            return Err(CliParseFailure::arguments());
        }
        filters.push(Filter {
            field: FieldId::DurationMs,
            predicate: Predicate::AtLeast,
            values: vec![FieldValue::Float(value)],
            ignore_case: false,
        });
    }
    Ok(())
}

fn parse_entity(value: &str) -> Result<EntityId, CliParseFailure> {
    value.parse().map_err(|_| CliParseFailure::arguments())
}

fn parse_name<T: FromStr>(value: &str) -> Result<T, CliParseFailure> {
    value.parse().map_err(|_| CliParseFailure::arguments())
}

fn validate_registry_name(value: String) -> Result<String, CliParseFailure> {
    let _: AdapterId = parse_name(&value)?;
    Ok(value)
}

fn validate_pij_id(value: &str) -> Result<String, CliParseFailure> {
    if value.is_empty()
        || value.starts_with('-')
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(CliParseFailure::pij_id());
    }
    Ok(value.to_owned())
}

fn absolute(path: PathBuf, context: &CliContext) -> Result<PathBuf, CliParseFailure> {
    let path = if path.is_absolute() {
        path
    } else if context.cwd.is_absolute() {
        context.cwd.join(path)
    } else {
        return Err(CliParseFailure::arguments());
    };
    if path.to_str().is_none() {
        return Err(CliParseFailure::arguments());
    }
    Ok(path)
}

impl QueryArgs {
    fn has_query_selector(&self) -> bool {
        self.pij.is_some()
            || self.repo.is_some()
            || self.source.is_some()
            || self.input.is_some()
            || self.has_query_option_besides_repo()
    }

    fn has_query_option_besides_repo(&self) -> bool {
        self.pij.is_some()
            || self.source.is_some()
            || self.input.is_some()
            || self.stdin_format.is_some()
            || self.repo_scope != "tree"
            || !self.harness.is_empty()
            || !self.exclude_harness.is_empty()
            || !self.source_adapter.is_empty()
            || !self.exclude_source_adapter.is_empty()
            || !self.session.is_empty()
            || !self.native_id.is_empty()
            || !self.model.is_empty()
            || !self.name.is_empty()
            || self.branch.is_some()
            || !self.parent.is_empty()
            || !self.role.is_empty()
            || !self.turn.is_empty()
            || !self.call.is_empty()
            || !self.contains.is_empty()
            || !self.regex.is_empty()
            || self.ignore_case
            || self.since.is_some()
            || self.until.is_some()
            || self.time_field.is_some()
            || self.include_undated
            || !self.columns.is_empty()
            || !self.sort.is_empty()
            || self.limit.is_some()
            || self.cursor.is_some()
            || self.range.is_some()
            || self.context_before != 0
            || self.context_after != 0
            || self.min_turns.is_some()
            || self.min_tool_calls.is_some()
            || !self.has_role.is_empty()
            || !self.has_tool.is_empty()
            || !self.has_tool_family.is_empty()
            || self.has_errors
            || !self.tool.is_empty()
            || !self.tool_family.is_empty()
            || !self.status.is_empty()
            || !self.exit_code.is_empty()
            || self.min_duration.is_some()
            || self.has_duration
            || !self.command_contains.is_empty()
            || !self.kind.is_empty()
            || self.part.is_some()
            || self.include_content
            || self.allow_partial
            || self.unresolved != "reject"
            || self.format.is_some()
            || self.csv_safety.is_some()
            || self.output.is_some()
            || self.max_output_bytes.is_some()
    }
}

/// Select diagnostic/presentation mode before argv is moved into [`parse`].
pub fn diagnostic_mode(args: &[OsString], stdout_is_terminal: bool) -> OutputMode {
    let mut json = false;
    let mut human = false;
    let mut raw_format = false;
    let diagnostic_command = args
        .iter()
        .skip(1)
        .filter_map(|arg| arg.to_str())
        .find(|arg| !arg.starts_with('-'))
        .is_some_and(|command| {
            matches!(
                command,
                "sources" | "sessions" | "turns" | "messages" | "tools" | "events"
            )
        });
    let mut expect_format = false;
    for arg in args.iter().skip(1).take_while(|arg| *arg != "--") {
        if expect_format {
            raw_format = matches!(arg.to_str(), Some("jsonl" | "csv" | "text" | "markdown"));
            expect_format = false;
            continue;
        }
        if arg == "--json" {
            json = true;
        } else if arg == "--human" {
            human = true;
        } else if arg == "--format" {
            expect_format = true;
        } else if let Some(format) = arg.to_str().and_then(|arg| arg.strip_prefix("--format=")) {
            raw_format = matches!(format, "jsonl" | "csv" | "text" | "markdown");
        }
    }
    if json {
        OutputMode::Json
    } else if human {
        OutputMode::Human
    } else if raw_format || diagnostic_command {
        OutputMode::JsonDiagnostic
    } else if stdout_is_terminal {
        OutputMode::Human
    } else {
        OutputMode::Json
    }
}
