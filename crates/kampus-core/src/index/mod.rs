//! Indexing orchestration
//!
//! Coordinates file crawling, parsing, and database writes.

pub mod full;
pub mod incremental;

pub use full::FullIndexer;
pub use incremental::IncrementalIndexer;

use crate::graph::writer::WriteStats;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IndexError {
    #[error("Crawler error: {0}")]
    Crawler(#[from] crate::crawler::CrawlerError),
    #[error("Parser error: {0}")]
    Parser(#[from] crate::parser::pool::ParserError),
    #[error("Extractor error: {0}")]
    Extractor(#[from] crate::parser::extractor::ExtractorError),
    #[error("Graph error: {0}")]
    Graph(#[from] crate::graph::GraphError),
    #[error("Git error: {0}")]
    Git(#[from] crate::git::GitError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for indexing operations
pub type IndexResult<T> = Result<T, IndexError>;

/// Statistics from an indexing operation
#[derive(Debug, Clone, Default)]
pub struct IndexingStats {
    /// Number of files discovered
    pub files_discovered: usize,
    /// Number of files parsed
    pub files_parsed: usize,
    /// Number of files skipped (already indexed)
    pub files_skipped: usize,
    /// Number of files that failed to parse
    pub files_failed: usize,
    /// Write statistics
    pub write_stats: WriteStats,
    /// Total duration
    pub duration: Duration,
}

impl std::fmt::Display for IndexingStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Indexing Statistics:")?;
        writeln!(f, "  Files discovered: {}", self.files_discovered)?;
        writeln!(f, "  Files parsed:     {}", self.files_parsed)?;
        writeln!(f, "  Files skipped:    {}", self.files_skipped)?;
        writeln!(f, "  Files failed:     {}", self.files_failed)?;
        writeln!(f, "  Symbols written:  {}", self.write_stats.symbols_written)?;
        writeln!(f, "  Duration:         {:.2?}", self.duration)?;
        Ok(())
    }
}
