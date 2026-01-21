//! File discovery and crawling module
//!
//! Uses the `ignore` crate to walk directories while respecting .gitignore files.

use crate::Language;
use ignore::{WalkBuilder, WalkState};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CrawlerError {
    #[error("Failed to walk directory: {0}")]
    WalkError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration for the file crawler
#[derive(Debug, Clone)]
pub struct CrawlerConfig {
    /// Root directory to crawl
    pub root: PathBuf,
    /// Languages to include (None = all supported)
    pub languages: Option<Vec<Language>>,
    /// Number of threads to use
    pub threads: usize,
    /// Respect .gitignore files
    pub git_ignore: bool,
    /// Additional patterns to ignore
    pub ignore_patterns: Vec<String>,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            languages: None,
            threads: num_cpus::get(),
            git_ignore: true,
            ignore_patterns: vec![
                "node_modules".to_string(),
                "target".to_string(),
                ".git".to_string(),
                "vendor".to_string(),
                "dist".to_string(),
                "build".to_string(),
                "__pycache__".to_string(),
                ".venv".to_string(),
                "venv".to_string(),
            ],
        }
    }
}

/// A discovered source file
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub language: Language,
}

/// File crawler that discovers source files
pub struct Crawler {
    config: CrawlerConfig,
}

impl Crawler {
    pub fn new(config: CrawlerConfig) -> Self {
        Self { config }
    }

    /// Crawl the directory and return all discovered source files
    pub fn crawl(&self) -> Result<Vec<SourceFile>, CrawlerError> {
        let (tx, rx) = mpsc::channel();

        let mut builder = WalkBuilder::new(&self.config.root);
        builder
            .git_ignore(self.config.git_ignore)
            .git_global(self.config.git_ignore)
            .git_exclude(self.config.git_ignore)
            .hidden(true)
            .threads(self.config.threads);

        // Add ignore patterns
        for pattern in &self.config.ignore_patterns {
            let mut override_builder = ignore::overrides::OverrideBuilder::new(&self.config.root);
            override_builder
                .add(&format!("!**/{}", pattern))
                .ok();
            if let Ok(overrides) = override_builder.build() {
                builder.overrides(overrides);
            }
        }

        // Get the languages filter
        let languages = self.config.languages.clone();

        builder.build_parallel().run(|| {
            let tx = tx.clone();
            let languages = languages.clone();

            Box::new(move |result| {
                if let Ok(entry) = result {
                    let path = entry.path();

                    // Skip directories
                    if !path.is_file() {
                        return WalkState::Continue;
                    }

                    // Get extension and detect language
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if let Some(language) = Language::from_extension(ext) {
                            // Filter by language if specified
                            if let Some(ref langs) = languages {
                                if !langs.contains(&language) {
                                    return WalkState::Continue;
                                }
                            }

                            let _ = tx.send(SourceFile {
                                path: path.to_path_buf(),
                                language,
                            });
                        }
                    }
                }
                WalkState::Continue
            })
        });

        // Drop the original sender to close the channel
        drop(tx);

        // Collect results
        let files: Vec<SourceFile> = rx.into_iter().collect();
        Ok(files)
    }

    /// Get the number of files for each language
    pub fn count_by_language(&self) -> Result<std::collections::HashMap<Language, usize>, CrawlerError> {
        let files = self.crawl()?;
        let mut counts = std::collections::HashMap::new();
        for file in files {
            *counts.entry(file.language).or_insert(0) += 1;
        }
        Ok(counts)
    }
}

/// Convenience function to crawl a directory
pub fn crawl_directory(
    root: impl AsRef<Path>,
    languages: Option<Vec<Language>>,
) -> Result<Vec<SourceFile>, CrawlerError> {
    let config = CrawlerConfig {
        root: root.as_ref().to_path_buf(),
        languages,
        ..Default::default()
    };
    Crawler::new(config).crawl()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("js"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("go"), Some(Language::Go));
        assert_eq!(Language::from_extension("cpp"), Some(Language::Cpp));
        assert_eq!(Language::from_extension("txt"), None);
    }
}
