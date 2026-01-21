//! Thread-local parser pool for efficient parallel parsing

use crate::Language;
use std::cell::RefCell;
use std::collections::HashMap;
use thiserror::Error;
use tree_sitter::Parser;

#[derive(Error, Debug)]
pub enum ParserError {
    #[error("Failed to set language for parser: {0}")]
    SetLanguage(String),
    #[error("Failed to parse file: {0}")]
    Parse(String),
    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),
}

// Thread-local storage for parsers (one per language per thread)
thread_local! {
    static PARSERS: RefCell<HashMap<Language, Parser>> = RefCell::new(HashMap::new());
}

/// Pool of tree-sitter parsers optimized for parallel processing
pub struct ParserPool;

impl ParserPool {
    /// Get or create a parser for the given language
    pub fn get_parser(language: Language) -> Result<(), ParserError> {
        PARSERS.with(|parsers| {
            let mut parsers = parsers.borrow_mut();
            if !parsers.contains_key(&language) {
                let mut parser = Parser::new();
                let ts_language = Self::get_tree_sitter_language(language)?;
                parser
                    .set_language(&ts_language)
                    .map_err(|e| ParserError::SetLanguage(e.to_string()))?;
                parsers.insert(language, parser);
            }
            Ok(())
        })
    }

    /// Parse source code with the appropriate language parser
    pub fn parse(
        language: Language,
        source: &[u8],
    ) -> Result<tree_sitter::Tree, ParserError> {
        Self::get_parser(language)?;

        PARSERS.with(|parsers| {
            let mut parsers = parsers.borrow_mut();
            let parser = parsers
                .get_mut(&language)
                .ok_or_else(|| ParserError::UnsupportedLanguage(language.to_string()))?;
            parser
                .parse(source, None)
                .ok_or_else(|| ParserError::Parse("Parser returned None".to_string()))
        })
    }

    /// Get the tree-sitter language for a Language enum
    fn get_tree_sitter_language(
        language: Language,
    ) -> Result<tree_sitter::Language, ParserError> {
        match language {
            Language::Python => Ok(tree_sitter_python::LANGUAGE.into()),
            Language::Rust => Ok(tree_sitter_rust::LANGUAGE.into()),
            Language::JavaScript => Ok(tree_sitter_javascript::LANGUAGE.into()),
            Language::TypeScript => Ok(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            Language::Go => Ok(tree_sitter_go::LANGUAGE.into()),
            Language::Cpp => Ok(tree_sitter_cpp::LANGUAGE.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_python() {
        let source = b"def hello(): pass";
        let tree = ParserPool::parse(Language::Python, source).unwrap();
        assert!(tree.root_node().child_count() > 0);
    }

    #[test]
    fn test_parse_rust() {
        let source = b"fn main() {}";
        let tree = ParserPool::parse(Language::Rust, source).unwrap();
        assert!(tree.root_node().child_count() > 0);
    }

    #[test]
    fn test_parse_javascript() {
        let source = b"function hello() {}";
        let tree = ParserPool::parse(Language::JavaScript, source).unwrap();
        assert!(tree.root_node().child_count() > 0);
    }
}
