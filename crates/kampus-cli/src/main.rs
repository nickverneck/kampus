//! Kampus CLI - Code indexing tool
//!
//! A tree-sitter based code indexer that creates a queryable knowledge graph.

mod commands;

use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "kampus")]
#[command(about = "A code indexing tool that creates a queryable knowledge graph")]
#[command(version)]
struct Cli {
    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// FalkorDB connection URI (default: redis://localhost:6379)
    #[arg(long, global = true, env = "KAMPUS_DB_URI")]
    db_uri: Option<String>,

    /// Graph name (default: kampus)
    #[arg(long, global = true, default_value = "kampus")]
    graph: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index the codebase
    Index {
        /// Root directory to index (default: current directory)
        #[arg(default_value = ".")]
        path: String,

        /// Number of parallel jobs
        #[arg(short, long)]
        jobs: Option<usize>,

        /// Languages to index (comma-separated: py,rs,ts,js,go,cpp)
        #[arg(short, long)]
        languages: Option<String>,

        /// Don't clear existing data before indexing
        #[arg(long)]
        no_clear: bool,
    },

    /// Incrementally update the index based on git changes
    Update {
        /// Root directory (default: current directory)
        #[arg(default_value = ".")]
        path: String,

        /// Git reference to compare against (default: last indexed commit)
        #[arg(long)]
        since: Option<String>,

        /// Show what would be updated without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Execute a Cypher query against the graph
    Query {
        /// The Cypher query to execute
        cypher: String,

        /// Output format (json, table)
        #[arg(short, long, default_value = "table")]
        format: String,
    },

    /// Find symbols by name pattern
    #[command(
        about = "Find symbols by name pattern",
        long_about = "Find symbols by name pattern (case-insensitive). Supports '*' wildcards.\n\nKinds:\n  function, class, struct, interface, method, trait, enum\n\nLanguages:\n  rs, py, ts, js, go, cpp\n\nExamples:\n  kampus find \"User*\"              # names starting with User\n  kampus find \"*product*\"           # names containing product\n  kampus find \"process_*\" --kind function --language rs\n  kampus find \"*product*\" --full-paths --limit 50\n\nTips for LLMs/agents:\n  - Always quote the pattern to prevent shell expansion.\n  - Use --full-paths to avoid truncated file paths.\n  - Use --kind/--language to narrow results and reduce noise.\n"
    )]
    Find {
        /// Symbol name pattern (supports * wildcards)
        pattern: String,

        /// Symbol kind to filter (function, class, struct, interface, method)
        /// Valid kinds: function, class, struct, interface, method, trait, enum
        #[arg(short, long, verbatim_doc_comment)]
        kind: Option<String>,

        /// Language to filter (rs, py, ts, js, go, cpp)
        #[arg(short, long, verbatim_doc_comment)]
        language: Option<String>,

        /// Maximum number of results
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,

        /// Show full file paths (no truncation in the table output)
        /// Useful for agents/LLMs that need to copy exact paths.
        #[arg(long, verbatim_doc_comment)]
        full_paths: bool,
    },

    /// Show the call graph for a function
    Calls {
        /// Function name to analyze
        function: String,

        /// Direction: callers, callees, or both
        #[arg(short, long, default_value = "both")]
        direction: String,

        /// Maximum depth to traverse
        #[arg(short = 'D', long, default_value = "3")]
        depth: u32,
    },

    /// Show index status and statistics
    Status {
        /// Show list of indexed files
        #[arg(long)]
        files: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    // Execute command
    match cli.command {
        Commands::Index {
            path,
            jobs,
            languages,
            no_clear,
        } => {
            commands::index::run(
                &path,
                jobs,
                languages.as_deref(),
                !no_clear,
                cli.db_uri.as_deref(),
                &cli.graph,
            )
            .await
        }
        Commands::Update {
            path,
            since,
            dry_run,
        } => {
            commands::update::run(
                &path,
                since.as_deref(),
                dry_run,
                cli.db_uri.as_deref(),
                &cli.graph,
            )
            .await
        }
        Commands::Query { cypher, format } => {
            commands::query::run(&cypher, &format, cli.db_uri.as_deref(), &cli.graph).await
        }
        Commands::Find {
            pattern,
            kind,
            language,
            limit,
            full_paths,
        } => {
            commands::find::run(
                &pattern,
                kind.as_deref(),
                language.as_deref(),
                limit,
                full_paths,
                cli.db_uri.as_deref(),
                &cli.graph,
            )
            .await
        }
        Commands::Calls {
            function,
            direction,
            depth,
        } => {
            commands::calls::run(
                &function,
                &direction,
                depth,
                cli.db_uri.as_deref(),
                &cli.graph,
            )
            .await
        }
        Commands::Status { files } => {
            commands::status::run(files, cli.db_uri.as_deref(), &cli.graph).await
        }
    }
}
