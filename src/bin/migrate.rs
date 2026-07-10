//! `llm-kernel-migrate-graph` — copy an llm-kernel graph between SQLite and
//! PostgreSQL backends through the shared `GraphBackend` trait (feature
//! `graph-pg`).
//!
//! Endpoints are given as `sqlite:<path>` (a file path) or a bare PostgreSQL
//! connstring (e.g. `postgresql://user@host/db`):
//!
//! ```text
//! llm-kernel-migrate-graph migrate --from sqlite:./graph.db --to postgresql://user@localhost/graph
//! llm-kernel-migrate-graph migrate --from postgresql://user@localhost/graph --to sqlite:./graph.db --dry-run
//! ```
//!
//! `--dry-run` enumerates the source and prints planned counts without opening
//! the target, so it works offline (no live PostgreSQL needed to inspect a
//! SQLite source).

use std::error::Error;
use std::path::PathBuf;

use clap::Parser;
use llm_kernel::graph::store::list_node_ids;
use llm_kernel::graph::{
    GraphBackend, GraphEdge, GraphNode, PgGraph, SqliteGraph, init_graph_schema, read_edges,
    read_nodes,
};
use rusqlite::Connection;

/// Cap for a single enumerate pass — effectively "all rows" for any real graph.
const LIST_LIMIT: usize = 5_000_000;

/// Command-line arguments for the migration tool.
#[derive(Parser, Debug)]
#[command(
    name = "llm-kernel-migrate-graph",
    about = "Migrate an llm-kernel graph between SQLite and PostgreSQL backends"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Supported subcommands.
#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Copy a graph from `--from` to `--to` (SQLite ↔ PostgreSQL).
    Migrate {
        /// Source: `sqlite:<path>` or a PostgreSQL connstring.
        #[arg(long)]
        from: String,
        /// Target: `sqlite:<path>` or a PostgreSQL connstring.
        #[arg(long)]
        to: String,
        /// Plan only — enumerate the source, do not write the target.
        #[arg(long)]
        dry_run: bool,
    },
}

/// A migration endpoint, parsed from the `--from` / `--to` flag.
enum Endpoint {
    /// A SQLite file at the given path.
    Sqlite(PathBuf),
    /// A PostgreSQL connstring.
    Postgres(String),
}

impl Endpoint {
    /// Parse a flag value: `sqlite:<path>` selects SQLite; anything else is a
    /// PostgreSQL connstring (`postgresql://…` or `key=value`).
    fn parse(s: &str) -> Result<Self, Box<dyn Error>> {
        if let Some(p) = s.strip_prefix("sqlite:") {
            Ok(Self::Sqlite(PathBuf::from(p)))
        } else {
            Ok(Self::Postgres(s.to_string()))
        }
    }

    /// Human-readable label (never echoes connection credentials).
    fn describe(&self) -> &'static str {
        match self {
            Self::Sqlite(_) => "sqlite",
            Self::Postgres(_) => "postgres",
        }
    }
}

/// Enumerate every node and edge from `from`.
fn read_source(from: &Endpoint) -> Result<(Vec<GraphNode>, Vec<GraphEdge>), Box<dyn Error>> {
    Ok(match from {
        Endpoint::Sqlite(p) => {
            let conn = Connection::open(p)?;
            init_graph_schema(&conn)?;
            let ids = list_node_ids(&conn)?;
            let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
            let nodes = read_nodes(&conn, &refs)?;
            let edges = read_edges(&conn, LIST_LIMIT)?;
            (nodes, edges)
        }
        Endpoint::Postgres(c) => {
            let pg = PgGraph::connect(c)?;
            let nodes = pg.list_nodes(LIST_LIMIT)?;
            let edges = pg.list_edges(LIST_LIMIT)?;
            (nodes, edges)
        }
    })
}

/// Write every node and edge to `to` through the trait (idempotent upserts).
fn write_target(
    to: &Endpoint,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<(), Box<dyn Error>> {
    let backend: Box<dyn GraphBackend> = match to {
        Endpoint::Sqlite(p) => Box::new(SqliteGraph::open(p)?),
        Endpoint::Postgres(c) => Box::new(PgGraph::connect(c)?),
    };
    for n in nodes {
        backend.upsert_node(n)?;
    }
    backend.append_edges(edges)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let Command::Migrate { from, to, dry_run } = cli.command;

    let from = Endpoint::parse(&from)?;
    let to = Endpoint::parse(&to)?;

    let (nodes, edges) = read_source(&from)?;
    eprintln!(
        "source ({}) -> {} nodes, {} edges",
        from.describe(),
        nodes.len(),
        edges.len()
    );

    if dry_run {
        eprintln!("dry-run: would write to {}", to.describe());
        if let Some(n) = nodes.first() {
            eprintln!("sample node: id={} title={}", n.id, n.title);
        }
        return Ok(());
    }

    write_target(&to, &nodes, &edges)?;
    eprintln!(
        "migrated {} nodes + {} edges to {}",
        nodes.len(),
        edges.len(),
        to.describe()
    );
    Ok(())
}
