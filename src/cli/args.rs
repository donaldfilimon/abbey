use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    Pwsh,
    Elvish,
}

#[derive(Debug, Clone, Subcommand)]
pub enum GenerateCmd {
    /// Image generation / edit (same as `abbey imagine`)
    Image {
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        #[arg(long)]
        aspect: Option<String>,
        #[arg(long = "edit", value_name = "PATH")]
        edit: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Video generation (best-effort; requires an agent/MCP video tool)
    Video {
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum MemoryCmd {
    /// Chat id + recent history (default)
    Chat,
    /// Store a memory record
    Put {
        summary: String,
        #[arg(long, default_value = "stm")]
        retention: String,
        #[arg(long, default_value = "")]
        payload: String,
        #[arg(long, default_value = "abbey cli")]
        provenance: String,
        /// Subject tag — the 3-D map's topic axis groups by this (repeatable)
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Source category such as session, route, or import.
        #[arg(long, default_value = "session")]
        source: String,
        /// Source-specific stable reference.
        #[arg(long)]
        source_ref: Option<String>,
        /// Project root/name override (defaults to the current Git root).
        #[arg(long)]
        project: Option<String>,
        /// Record timestamp in RFC 3339 form.
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// Get by id
    Get { id: String },
    /// Keyword search
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Promote retention layer
    Promote {
        id: String,
        #[arg(default_value = "ltm")]
        retention: String,
    },
    /// Mark a memory obsolete (never deletes — provenance is preserved)
    Invalidate { id: String },
    /// Replace a memory with a corrected one, marking the old obsolete
    Supersede {
        /// Record being replaced
        old_id: String,
        /// Summary of the replacement record
        summary: String,
        #[arg(long, default_value = "stm")]
        retention: String,
        #[arg(long, default_value = "")]
        payload: String,
        #[arg(long, default_value = "abbey cli supersede")]
        provenance: String,
        /// Subject tag for the replacement (repeatable)
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Source category such as session, route, or import.
        #[arg(long, default_value = "session")]
        source: String,
        /// Source-specific stable reference.
        #[arg(long)]
        source_ref: Option<String>,
        /// Project root/name override (defaults to the current Git root).
        #[arg(long)]
        project: Option<String>,
        /// Replacement timestamp in RFC 3339 form.
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// Reflection report (duplicates / low confidence / superseded)
    Reflect,
    /// 3-D memory map: topic × recency × consolidation
    Map {
        #[arg(long, default_value_t = 40)]
        limit: usize,
        /// Only this retention layer
        #[arg(long)]
        layer: Option<String>,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Memories nearest another one in the 3-D map
    Near {
        id: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Lexical similarity search (feature-hash cosine — not learned semantics)
    ///
    /// Ranks by shared character n-grams, so it tolerates typos and word order
    /// where `search` (substring) misses. Pass `--id` to anchor on a record.
    Similar {
        /// Free-text query; omit when using --id
        #[arg(default_value = "")]
        query: String,
        /// Anchor on an existing record instead of free text
        #[arg(long)]
        id: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Learned semantic search in the explicitly selected embedding space
    Semantic {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Copy every memory record between backends, obsolete ones included
    Migrate {
        /// Source backend (`sqlite` | `wdbx`)
        #[arg(long)]
        from: String,
        /// Destination backend (`sqlite` | `wdbx`). Must be empty.
        #[arg(long)]
        to: String,
        /// Report what would move and write nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Export layer as JSONL (train_candidate requires provenance on records)
    Export {
        #[arg(long, default_value = "ltm")]
        layer: String,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Inspect or explicitly populate the selected semantic embedding space
    #[command(visible_alias = "embedding", visible_alias = "vector")]
    Embed {
        /// Record id, or `status`; omit only with --all
        #[arg(conflicts_with = "all")]
        id: Option<String>,
        /// Backfill every pending or stale record in the selected space
        #[arg(long)]
        all: bool,
        /// Recompute an embedding even when the current mapping is fresh
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum MeshCmd {
    /// Show the ABI binary resolution and local-proof claim boundary
    Status,
    /// Show the bounded node counts accepted by local-demo
    Nodes,
    /// Run ABI's authenticated loopback multi-process proof (Unix only)
    #[command(name = "local-demo")]
    LocalDemo {
        /// Independent ABI processes to spawn (3 through 9)
        #[arg(long, default_value_t = 3, value_parser = parse_mesh_nodes)]
        nodes: usize,
        /// Emit the parsed proof as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum DaemonCmd {
    /// Show the daemon runtime and advertised read-only capabilities
    Status {
        /// Emit the typed application event as JSON
        #[arg(long)]
        json: bool,
    },
    /// Query the daemon's canonical capability claims
    Claims {
        /// Select one evidence status
        #[arg(long, value_enum)]
        status: Option<DaemonClaimStatus>,
        /// Case-insensitive substring filter over claim text
        #[arg(long, value_name = "TEXT")]
        contains: Option<String>,
        /// Emit the typed application event as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read a bounded, sanitized tail of the persona/role routing audit log
    ///
    /// The working directory of each decision is reported as an opaque
    /// `ws-<digest>` label — never as a filesystem path.
    Routes {
        /// Maximum decisions to return (1 through 50)
        #[arg(long, default_value_t = crate::app_core::MAX_ROUTE_AUDIT_PAGE, value_parser = clap::value_parser!(u16).range(1..=i64::from(crate::app_core::MAX_ROUTE_AUDIT_PAGE)))]
        limit: u16,
        /// Emit the typed application event as JSON
        #[arg(long)]
        json: bool,
    },
    /// Explicitly negotiate the protocol-v3 model-read capability
    Negotiate {
        /// Emit the typed protocol-v3 event as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read the bounded startup-owned protocol-v3 model inventory
    Models {
        /// Return records after this zero-based fixed-watermark cursor
        #[arg(long, default_value_t = 0)]
        after: u64,
        /// Keep paging against this previously returned watermark
        #[arg(long)]
        through: Option<u64>,
        /// Maximum records to return (1 through 32)
        #[arg(long, default_value_t = crate::app_core::MAX_V3_PAGE, value_parser = clap::value_parser!(u16).range(1..=i64::from(crate::app_core::MAX_V3_PAGE)))]
        limit: u16,
        /// Emit the typed protocol-v3 event as JSON
        #[arg(long)]
        json: bool,
    },
    /// Submit, inspect, cancel, or page events for bounded protocol-v2 runs
    Run {
        #[command(subcommand)]
        cmd: crate::run_control::RunControlCliCommand,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DaemonClaimStatus {
    Current,
    Partial,
    Proposed,
    Blocked,
    #[value(name = "oos", alias = "out-of-scope", alias = "out_of_scope")]
    OutOfScope,
    Failed,
    Revoked,
    Superseded,
    Expired,
}

fn parse_mesh_nodes(value: &str) -> Result<usize, String> {
    let nodes = value
        .parse::<usize>()
        .map_err(|_| "nodes must be an integer from 3 through 9".to_string())?;
    if (3..=9).contains(&nodes) {
        Ok(nodes)
    } else {
        Err("nodes must be from 3 through 9".into())
    }
}

/// Filters shared by memory query surfaces.
#[derive(Debug, Clone, Default, Args)]
pub struct MemoryFilterArgs {
    /// Subject tag.
    #[arg(long)]
    pub tag: Option<String>,
    /// Source category.
    #[arg(long)]
    pub source: Option<String>,
    /// Exact source reference.
    #[arg(long)]
    pub source_ref: Option<String>,
    /// Exact project identifier/root.
    #[arg(long)]
    pub project: Option<String>,
    /// Inclusive RFC 3339 lower timestamp bound.
    #[arg(long)]
    pub since: Option<String>,
    /// Inclusive RFC 3339 upper timestamp bound.
    #[arg(long)]
    pub until: Option<String>,
}
