//! Kampus Core Library
//!
//! Core functionality for the Kampus code indexing tool.
//! Provides tree-sitter parsing, symbol extraction, and graph database operations.

pub mod crawler;
pub mod git;
pub mod graph;
pub mod index;
pub mod parser;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Supported programming languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    Rust,
    JavaScript,
    TypeScript,
    Go,
    Cpp,
}

impl Language {
    /// Detect language from file extension
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "py" => Some(Language::Python),
            "rs" => Some(Language::Rust),
            "js" | "mjs" | "cjs" => Some(Language::JavaScript),
            "ts" | "tsx" => Some(Language::TypeScript),
            "go" => Some(Language::Go),
            "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hxx" | "h" => Some(Language::Cpp),
            _ => None,
        }
    }

    /// Get all file extensions for this language
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Language::Python => &["py"],
            Language::Rust => &["rs"],
            Language::JavaScript => &["js", "mjs", "cjs"],
            Language::TypeScript => &["ts", "tsx"],
            Language::Go => &["go"],
            Language::Cpp => &["cpp", "cc", "cxx", "c++", "hpp", "hxx", "h"],
        }
    }

    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            Language::Python => "Python",
            Language::Rust => "Rust",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Go => "Go",
            Language::Cpp => "C++",
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl std::str::FromStr for Language {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "python" | "py" => Ok(Language::Python),
            "rust" | "rs" => Ok(Language::Rust),
            "javascript" | "js" => Ok(Language::JavaScript),
            "typescript" | "ts" => Ok(Language::TypeScript),
            "go" => Ok(Language::Go),
            "cpp" | "c++" => Ok(Language::Cpp),
            _ => Err(format!("Unknown language: {}", s)),
        }
    }
}

/// Visibility of a symbol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
}

/// Type of symbol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Class,
    Struct,
    Interface,
    Module,
    Method,
    Trait,
    Enum,
    Constant,
    Variable,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => write!(f, "function"),
            SymbolKind::Class => write!(f, "class"),
            SymbolKind::Struct => write!(f, "struct"),
            SymbolKind::Interface => write!(f, "interface"),
            SymbolKind::Module => write!(f, "module"),
            SymbolKind::Method => write!(f, "method"),
            SymbolKind::Trait => write!(f, "trait"),
            SymbolKind::Enum => write!(f, "enum"),
            SymbolKind::Constant => write!(f, "constant"),
            SymbolKind::Variable => write!(f, "variable"),
        }
    }
}

/// A code symbol (function, class, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Unique identifier (file_path:name:start_line)
    pub id: String,
    /// Symbol name
    pub name: String,
    /// Kind of symbol
    pub kind: SymbolKind,
    /// File path where symbol is defined
    pub file_path: PathBuf,
    /// Starting line number (1-indexed)
    pub start_line: u32,
    /// Ending line number (1-indexed)
    pub end_line: u32,
    /// Function signature or declaration
    pub signature: Option<String>,
    /// Visibility
    pub visibility: Visibility,
    /// Whether the function is async
    pub is_async: bool,
    /// Documentation string
    pub docstring: Option<String>,
    /// LLM-generated summary (Phase 2)
    pub summary: Option<String>,
    /// Language of the symbol
    pub language: Language,
    /// Parent symbol ID (for methods in classes)
    pub parent_id: Option<String>,
}

impl Symbol {
    /// Create a unique ID for this symbol
    pub fn generate_id(file_path: &std::path::Path, name: &str, start_line: u32) -> String {
        format!("{}:{}:{}", file_path.display(), name, start_line)
    }
}

/// An import statement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    /// File containing the import
    pub source_file: PathBuf,
    /// Module or file being imported
    pub target: String,
    /// Alias if any
    pub alias: Option<String>,
    /// Specific items imported (for `from x import y, z`)
    pub items: Vec<String>,
    /// Line number of import
    pub line: u32,
}

/// A function call reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Call {
    /// ID of calling function
    pub caller_id: String,
    /// Name of called function
    pub callee_name: String,
    /// Line number where call occurs
    pub call_site_line: u32,
}

/// An inheritance relationship
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inheritance {
    /// ID of child class/struct
    pub child_id: String,
    /// Name of parent class/interface
    pub parent_name: String,
}

/// All extracted data from a file
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileSymbols {
    /// File path
    pub file_path: PathBuf,
    /// Detected language
    pub language: Option<Language>,
    /// Content hash (SHA256)
    pub content_hash: String,
    /// Line count
    pub line_count: u32,
    /// Extracted symbols
    pub symbols: Vec<Symbol>,
    /// Import statements
    pub imports: Vec<Import>,
    /// Function calls
    pub calls: Vec<Call>,
    /// Inheritance relationships
    pub inheritance: Vec<Inheritance>,
}

/// Index statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    pub total_files: usize,
    pub files_by_language: std::collections::HashMap<Language, usize>,
    pub total_symbols: usize,
    pub symbols_by_kind: std::collections::HashMap<SymbolKind, usize>,
    pub total_calls: usize,
    pub total_imports: usize,
    pub last_indexed_commit: Option<String>,
}
