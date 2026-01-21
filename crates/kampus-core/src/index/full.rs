//! Full indexing pipeline
//!
//! Performs a complete index of the codebase using parallel processing.

use super::{IndexResult, IndexingStats};
use crate::crawler::{Crawler, CrawlerConfig, SourceFile};
use crate::graph::writer::GraphWriter;
use crate::graph::GraphSchema;
use crate::parser::extractor::SymbolExtractor;
use crate::parser::pool::ParserPool;
use crate::{FileSymbols, Language};
use rayon::prelude::*;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;
use tracing::{debug, warn};

/// Configuration for full indexing
#[derive(Debug, Clone)]
pub struct FullIndexConfig {
    /// Root directory to index
    pub root: std::path::PathBuf,
    /// Languages to index (None = all)
    pub languages: Option<Vec<Language>>,
    /// Number of threads for parsing
    pub threads: usize,
    /// Whether to clear existing data first
    pub clear_existing: bool,
    /// FalkorDB connection URI
    pub db_uri: Option<String>,
    /// Graph name
    pub graph_name: String,
}

impl Default for FullIndexConfig {
    fn default() -> Self {
        Self {
            root: std::path::PathBuf::from("."),
            languages: None,
            threads: num_cpus::get(),
            clear_existing: true,
            db_uri: None,
            graph_name: "kampus".to_string(),
        }
    }
}

/// Full indexer that processes the entire codebase
pub struct FullIndexer {
    config: FullIndexConfig,
}

impl FullIndexer {
    pub fn new(config: FullIndexConfig) -> Self {
        Self { config }
    }

    /// Run the full indexing pipeline
    pub async fn run(&self) -> IndexResult<IndexingStats> {
        let start = Instant::now();
        let mut stats = IndexingStats::default();

        println!("Indexing {:?}", self.config.root);
        println!("Using {} threads\n", self.config.threads);

        // Connect to database
        print!("Connecting to database...");
        let _ = io::stdout().flush();
        let connect_start = Instant::now();

        let schema = GraphSchema::connect(
            self.config.db_uri.as_deref(),
            &self.config.graph_name,
        )
        .await?;

        // Initialize schema (create indexes)
        schema.initialize().await?;
        println!(" connected in {:.2?}", connect_start.elapsed());

        // Clear existing data if requested
        if self.config.clear_existing {
            print!("Clearing existing data...");
            let _ = io::stdout().flush();
            let clear_start = Instant::now();
            schema.clear().await?;
            println!(" done in {:.2?}", clear_start.elapsed());
        }

        // Discover files
        print!("Discovering files...");
        let _ = io::stdout().flush();
        let discover_start = Instant::now();

        let crawler_config = CrawlerConfig {
            root: self.config.root.clone(),
            languages: self.config.languages.clone(),
            threads: self.config.threads,
            ..Default::default()
        };
        let crawler = Crawler::new(crawler_config);
        let files = crawler.crawl()?;
        stats.files_discovered = files.len();

        println!(
            " found {} files in {:.2?}",
            files.len(),
            discover_start.elapsed()
        );

        // Parse files in parallel
        let parse_start = Instant::now();
        let total_files = files.len();
        print!("Parsing files... 0/{} (0%)", total_files);
        let _ = io::stdout().flush();

        let (tx, rx) = mpsc::channel::<FileSymbols>();
        let failed_count = Arc::new(AtomicUsize::new(0));

        // Configure rayon thread pool
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.config.threads)
            .build()
            .unwrap();

        let root = self.config.root.clone();
        let failed_count_clone = Arc::clone(&failed_count);

        pool.spawn(move || {
            files.par_iter().for_each_with(tx, |tx, file| {
                match parse_file(file, &root) {
                    Ok(symbols) => {
                        let _ = tx.send(symbols);
                    }
                    Err(e) => {
                        warn!("Failed to parse {:?}: {}", file.path, e);
                        failed_count_clone.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        });

        // Collect parsed results with progress updates
        let mut file_symbols: Vec<FileSymbols> = Vec::with_capacity(total_files);
        let mut last_update = Instant::now();

        for symbols in rx {
            file_symbols.push(symbols);

            // Update progress every 100ms
            if last_update.elapsed().as_millis() >= 100 {
                let done = file_symbols.len();
                let percent = (done * 100) / total_files.max(1);
                let elapsed = parse_start.elapsed().as_secs_f64();
                let rate = done as f64 / elapsed.max(0.001);
                let remaining = (total_files - done) as f64 / rate.max(0.001);

                print!(
                    "\rParsing files... {}/{} ({}%) - {:.1} files/sec, ~{:.1}s remaining    ",
                    done, total_files, percent, rate, remaining
                );
                let _ = io::stdout().flush();
                last_update = Instant::now();
            }
        }

        // Final progress update
        let parse_elapsed = parse_start.elapsed();
        stats.files_failed = failed_count.load(Ordering::Relaxed);
        println!(
            "\rParsing files... {}/{} (100%) - completed in {:.2?}                    ",
            file_symbols.len(), total_files, parse_elapsed
        );

        stats.files_parsed = file_symbols.len();

        // Write to database
        print!("Writing to database...");
        let _ = io::stdout().flush();
        let write_start = Instant::now();

        let writer = GraphWriter::new(schema);
        stats.write_stats = writer.write_files(file_symbols).await?;

        println!(
            " {} symbols in {:.2?}",
            stats.write_stats.symbols_written,
            write_start.elapsed()
        );

        // Store the current git commit if in a git repo
        if let Ok(git) = crate::git::GitDiff::open(&self.config.root) {
            if let Ok(commit) = git.head_commit() {
                writer
                    .schema()
                    .set_metadata("last_indexed_commit", &commit)
                    .await?;
                println!("Stored commit: {}", &commit[..12.min(commit.len())]);
            }
        }

        stats.duration = start.elapsed();
        println!("\nIndexing complete in {:.2?}", stats.duration);

        Ok(stats)
    }
}

/// Parse a single file and extract symbols
fn parse_file(file: &SourceFile, root: &Path) -> IndexResult<FileSymbols> {
    debug!("Parsing {:?}", file.path);

    // Read file contents
    let source = std::fs::read(&file.path)?;

    // Parse with tree-sitter
    let tree = ParserPool::parse(file.language, &source)?;

    // Extract symbols
    let relative_path = file
        .path
        .strip_prefix(root)
        .unwrap_or(&file.path);

    let file_symbols = SymbolExtractor::extract(&tree, &source, relative_path, file.language)?;

    Ok(file_symbols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_index_config_default() {
        let config = FullIndexConfig::default();
        assert!(config.clear_existing);
        assert!(config.languages.is_none());
    }
}
