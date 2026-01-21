//! Symbol extraction from tree-sitter ASTs

use crate::parser::languages::{
    CppExtractor, GoExtractor, JavaScriptExtractor, PythonExtractor, RustExtractor,
    TypeScriptExtractor,
};
use crate::{Call, FileSymbols, Import, Inheritance, Language, Symbol};
use std::path::Path;
use thiserror::Error;
use tree_sitter::Tree;

#[derive(Error, Debug)]
pub enum ExtractorError {
    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),
    #[error("Extraction failed: {0}")]
    ExtractionFailed(String),
}

/// Trait for language-specific symbol extraction
pub trait LanguageExtractor: Send + Sync {
    /// Extract symbols (functions, classes, etc.) from the AST
    fn extract_symbols(
        &self,
        tree: &Tree,
        source: &[u8],
        file_path: &Path,
    ) -> Vec<Symbol>;

    /// Extract import statements
    fn extract_imports(
        &self,
        tree: &Tree,
        source: &[u8],
        file_path: &Path,
    ) -> Vec<Import>;

    /// Extract function calls within functions
    fn extract_calls(
        &self,
        tree: &Tree,
        source: &[u8],
        symbols: &[Symbol],
    ) -> Vec<Call>;

    /// Extract inheritance relationships
    fn extract_inheritance(
        &self,
        tree: &Tree,
        source: &[u8],
        symbols: &[Symbol],
    ) -> Vec<Inheritance>;
}

/// Main extractor that delegates to language-specific extractors
pub struct SymbolExtractor;

impl SymbolExtractor {
    /// Extract all symbols and relationships from a file
    pub fn extract(
        tree: &Tree,
        source: &[u8],
        file_path: &Path,
        language: Language,
    ) -> Result<FileSymbols, ExtractorError> {
        let extractor = Self::get_extractor(language)?;

        let symbols = extractor.extract_symbols(tree, source, file_path);
        let imports = extractor.extract_imports(tree, source, file_path);
        let calls = extractor.extract_calls(tree, source, &symbols);
        let inheritance = extractor.extract_inheritance(tree, source, &symbols);

        let content_hash = Self::compute_hash(source);
        let line_count = source.iter().filter(|&&b| b == b'\n').count() as u32 + 1;

        Ok(FileSymbols {
            file_path: file_path.to_path_buf(),
            language: Some(language),
            content_hash,
            line_count,
            symbols,
            imports,
            calls,
            inheritance,
        })
    }

    fn get_extractor(language: Language) -> Result<Box<dyn LanguageExtractor>, ExtractorError> {
        match language {
            Language::Python => Ok(Box::new(PythonExtractor)),
            Language::Rust => Ok(Box::new(RustExtractor)),
            Language::JavaScript => Ok(Box::new(JavaScriptExtractor)),
            Language::TypeScript => Ok(Box::new(TypeScriptExtractor)),
            Language::Go => Ok(Box::new(GoExtractor)),
            Language::Cpp => Ok(Box::new(CppExtractor)),
        }
    }

    fn compute_hash(source: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(source);
        hex::encode(hasher.finalize())
    }
}

/// Helper to get text from a node
pub fn node_text<'a>(node: tree_sitter::Node<'a>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// Helper to find the first child of a specific kind
pub fn find_child<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return Some(child);
        }
    }
    None
}

/// Helper to find all children of a specific kind
pub fn find_children<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Vec<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .collect()
}

/// Helper to recursively find all nodes of a specific kind
pub fn find_all_nodes<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
    results: &mut Vec<tree_sitter::Node<'a>>,
) {
    if node.kind() == kind {
        results.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_all_nodes(child, kind, results);
    }
}

/// Helper to get docstring from preceding comment/string
pub fn extract_docstring<'a>(
    node: tree_sitter::Node<'a>,
    source: &'a [u8],
    comment_kinds: &[&str],
) -> Option<String> {
    // Look for preceding sibling that is a comment
    if let Some(prev) = node.prev_sibling() {
        if comment_kinds.contains(&prev.kind()) {
            return Some(node_text(prev, source).to_string());
        }
    }
    None
}
