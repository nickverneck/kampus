//! Update command implementation

use kampus_core::index::incremental::{IncrementalConfig, IncrementalIndexer};
use std::path::PathBuf;

pub async fn run(
    path: &str,
    since: Option<&str>,
    dry_run: bool,
    db_uri: Option<&str>,
    graph_name: &str,
) -> anyhow::Result<()> {
    let config = IncrementalConfig {
        root: PathBuf::from(path),
        languages: None,
        since: since.map(String::from),
        dry_run,
        db_uri: db_uri.map(String::from),
        graph_name: graph_name.to_string(),
    };

    let indexer = IncrementalIndexer::new(config);
    let stats = indexer.run().await?;

    println!("\n{}", stats);
    Ok(())
}
