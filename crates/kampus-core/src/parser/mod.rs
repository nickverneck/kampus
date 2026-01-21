//! Tree-sitter parsing module
//!
//! Provides thread-safe parser pool and language-specific extractors.

pub mod extractor;
pub mod languages;
pub mod pool;

pub use extractor::SymbolExtractor;
pub use pool::ParserPool;
