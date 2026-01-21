//! Incremental indexing based on git changes
//!
//! Only re-indexes files that have changed since the last index.

use super::{IndexError, IndexResult, IndexingStats};
use crate::git::{ChangeKind, ChangedFile, GitDiff};
use crate::graph::writer::GraphWriter;
use crate::graph::GraphSchema;
use crate::parser::extractor::SymbolExtractor;
use crate::parser::pool::ParserPool;
use crate::Language;
use std::io::{self, Write};
use std::path::Path;
use std::time::Instant;
use tracing::{debug, warn};

/// Configuration for incremental indexing
#[derive(Debug, Clone)]
pub struct IncrementalConfig {
    /// Root directory
    pub root: std::path::PathBuf,
    /// Languages to index
    pub languages: Option<Vec<Language>>,
    /// Reference to compare against (None = last indexed commit)
    pub since: Option<String>,
    /// Dry run (don't write to database)
    pub dry_run: bool,
    /// FalkorDB connection URI
    pub db_uri: Option<String>,
    /// Graph name
    pub graph_name: String,
}

impl Default for IncrementalConfig {
    fn default() -> Self {
        Self {
            root: std::path::PathBuf::from("."),
            languages: None,
            since: None,
            dry_run: false,
            db_uri: None,
            graph_name: "kampus".to_string(),
        }
    }
}

/// Incremental indexer that only processes changed files
pub struct IncrementalIndexer {
    config: IncrementalConfig,
}

impl IncrementalIndexer {
    pub fn new(config: IncrementalConfig) -> Self {
        Self { config }
    }

    /// Run incremental indexing
    pub async fn run(&self) -> IndexResult<IndexingStats> {
        let start = Instant::now();
        let mut stats = IndexingStats::default();

        println!("Incremental update of {:?}", self.config.root);

        // Open git repository
        let git = GitDiff::open(&self.config.root)?;

        // Connect to database
        print!("Connecting to database...");
        let _ = io::stdout().flush();
        let connect_start = Instant::now();

        let schema = GraphSchema::connect(
            self.config.db_uri.as_deref(),
            &self.config.graph_name,
        )
        .await?;

        println!(" connected in {:.2?}", connect_start.elapsed());

        // Get the reference to compare against
        let since = match &self.config.since {
            Some(ref_name) => ref_name.clone(),
            None => {
                // Get last indexed commit from database
                schema
                    .get_metadata("last_indexed_commit")
                    .await?
                    .ok_or_else(|| {
                        IndexError::Git(crate::git::GitError::InvalidRef(
                            "No previous index found. Run full index first.".to_string(),
                        ))
                    })?
            }
        };

        println!("Comparing against: {}...", &since[..12.min(since.len())]);

        // Get changed files
        let changes = git.changes_since(&since)?;
        let changes = self.filter_changes(changes);
        stats.files_discovered = changes.len();

        println!("Found {} changed files\n", changes.len());

        if self.config.dry_run {
            println!("Dry run - not writing to database:");
            for change in &changes {
                println!("  {:?}: {:?}", change.kind, change.path);
            }
            stats.duration = start.elapsed();
            return Ok(stats);
        }

        // Process changes
        let writer = GraphWriter::new(schema);
        let total_changes = changes.len();

        for (i, change) in changes.into_iter().enumerate() {
            print!(
                "\rProcessing {}/{} ({:.0}%)... ",
                i + 1,
                total_changes,
                ((i + 1) as f64 / total_changes.max(1) as f64) * 100.0
            );
            let _ = io::stdout().flush();

            match change.kind {
                ChangeKind::Deleted => {
                    let path_str = change.path.to_string_lossy();
                    writer.schema().delete_file(&path_str).await?;
                }
                ChangeKind::Renamed => {
                    // Delete old path, add new path
                    if let Some(ref old_path) = change.old_path {
                        let old_path_str = old_path.to_string_lossy();
                        writer.schema().delete_file(&old_path_str).await?;
                    }

                    // Parse and add the new path
                    match self.parse_and_write(&change.path, &writer).await {
                        Ok(_) => stats.files_parsed += 1,
                        Err(e) => {
                            warn!("Failed to parse {:?}: {}", change.path, e);
                            stats.files_failed += 1;
                        }
                    }
                }
                ChangeKind::Added | ChangeKind::Modified => {
                    // Delete old data for modified files
                    if change.kind == ChangeKind::Modified {
                        let path_str = change.path.to_string_lossy();
                        writer.schema().delete_file(&path_str).await?;
                    }

                    // Parse and add
                    match self.parse_and_write(&change.path, &writer).await {
                        Ok(_) => stats.files_parsed += 1,
                        Err(e) => {
                            warn!("Failed to parse {:?}: {}", change.path, e);
                            stats.files_failed += 1;
                        }
                    }
                }
            }
        }

        if total_changes > 0 {
            println!("\rProcessing {}/{} (100%) - done                    ", total_changes, total_changes);
        }

        // Update last indexed commit
        let new_commit = git.head_commit()?;
        writer
            .schema()
            .set_metadata("last_indexed_commit", &new_commit)
            .await?;
        println!("Updated commit: {}", &new_commit[..12.min(new_commit.len())]);

        stats.duration = start.elapsed();
        println!("\nIncremental update complete in {:.2?}", stats.duration);

        Ok(stats)
    }

    /// Filter changes to only include supported languages
    fn filter_changes(&self, changes: Vec<ChangedFile>) -> Vec<ChangedFile> {
        changes
            .into_iter()
            .filter(|change| {
                // Check if file has a supported extension
                let ext = change
                    .path
                    .extension()
                    .and_then(|e| e.to_str());

                if let Some(ext) = ext {
                    if let Some(lang) = Language::from_extension(ext) {
                        // Filter by language if specified
                        if let Some(ref languages) = self.config.languages {
                            return languages.contains(&lang);
                        }
                        return true;
                    }
                }
                false
            })
            .collect()
    }

    /// Parse a file and write to database
    async fn parse_and_write(
        &self,
        path: &Path,
        writer: &GraphWriter,
    ) -> IndexResult<()> {
        let full_path = self.config.root.join(path);

        // Detect language
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| {
                IndexError::Parser(crate::parser::pool::ParserError::UnsupportedLanguage(
                    "Unknown extension".to_string(),
                ))
            })?;

        let language = Language::from_extension(ext).ok_or_else(|| {
            IndexError::Parser(crate::parser::pool::ParserError::UnsupportedLanguage(
                ext.to_string(),
            ))
        })?;

        debug!("Parsing {:?} as {:?}", path, language);

        // Read and parse
        let source = std::fs::read(&full_path)?;
        let tree = ParserPool::parse(language, &source)?;
        let file_symbols = SymbolExtractor::extract(&tree, &source, path, language)?;

        // Write to database
        writer.write_file(&file_symbols).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incremental_config_default() {
        let config = IncrementalConfig::default();
        assert!(!config.dry_run);
        assert!(config.since.is_none());
    }
}
