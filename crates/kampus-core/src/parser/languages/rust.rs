//! Rust language extractor

use crate::parser::extractor::{
    extract_docstring, find_all_nodes, find_child, find_children, node_text, LanguageExtractor,
};
use crate::{Call, Import, Inheritance, Language, Symbol, SymbolKind, Visibility};
use std::path::Path;
use tree_sitter::Tree;

pub struct RustExtractor;

impl LanguageExtractor for RustExtractor {
    fn extract_symbols(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let root = tree.root_node();

        // Extract functions
        let mut func_nodes = Vec::new();
        find_all_nodes(root, "function_item", &mut func_nodes);
        for node in func_nodes {
            if let Some(symbol) = self.extract_function(node, source, file_path, None) {
                symbols.push(symbol);
            }
        }

        // Extract structs
        let mut struct_nodes = Vec::new();
        find_all_nodes(root, "struct_item", &mut struct_nodes);
        for node in struct_nodes {
            if let Some(symbol) = self.extract_struct(node, source, file_path) {
                symbols.push(symbol);
            }
        }

        // Extract enums
        let mut enum_nodes = Vec::new();
        find_all_nodes(root, "enum_item", &mut enum_nodes);
        for node in enum_nodes {
            if let Some(symbol) = self.extract_enum(node, source, file_path) {
                symbols.push(symbol);
            }
        }

        // Extract traits
        let mut trait_nodes = Vec::new();
        find_all_nodes(root, "trait_item", &mut trait_nodes);
        for node in trait_nodes {
            if let Some(symbol) = self.extract_trait(node, source, file_path) {
                symbols.push(symbol);
            }
        }

        // Extract impl blocks and their methods
        let mut impl_nodes = Vec::new();
        find_all_nodes(root, "impl_item", &mut impl_nodes);
        for node in impl_nodes {
            // Get the type being implemented
            let type_name = find_child(node, "type_identifier")
                .or_else(|| find_child(node, "generic_type"))
                .map(|n| node_text(n, source));

            if let Some(body) = find_child(node, "declaration_list") {
                for func in find_children(body, "function_item") {
                    if let Some(mut method) = self.extract_function(func, source, file_path, None) {
                        method.kind = SymbolKind::Method;
                        if let Some(type_name) = type_name {
                            // Try to find parent symbol
                            let parent_id = symbols
                                .iter()
                                .find(|s| {
                                    (s.kind == SymbolKind::Struct || s.kind == SymbolKind::Enum)
                                        && s.name == type_name
                                })
                                .map(|s| s.id.clone());
                            method.parent_id = parent_id;
                        }
                        symbols.push(method);
                    }
                }
            }
        }

        symbols
    }

    fn extract_imports(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Import> {
        let mut imports = Vec::new();
        let root = tree.root_node();

        // use declarations
        let mut use_nodes = Vec::new();
        find_all_nodes(root, "use_declaration", &mut use_nodes);

        for node in use_nodes {
            if let Some(use_clause) = node.child(1) {
                let (target, items) = self.parse_use_clause(use_clause, source);
                imports.push(Import {
                    source_file: file_path.to_path_buf(),
                    target,
                    alias: None,
                    items,
                    line: node.start_position().row as u32 + 1,
                });
            }
        }

        // extern crate
        let mut extern_nodes = Vec::new();
        find_all_nodes(root, "extern_crate_declaration", &mut extern_nodes);
        for node in extern_nodes {
            if let Some(name) = find_child(node, "identifier") {
                imports.push(Import {
                    source_file: file_path.to_path_buf(),
                    target: node_text(name, source).to_string(),
                    alias: None,
                    items: Vec::new(),
                    line: node.start_position().row as u32 + 1,
                });
            }
        }

        imports
    }

    fn extract_calls(&self, tree: &Tree, source: &[u8], symbols: &[Symbol]) -> Vec<Call> {
        let mut calls = Vec::new();
        let root = tree.root_node();

        for symbol in symbols.iter().filter(|s| {
            s.kind == SymbolKind::Function || s.kind == SymbolKind::Method
        }) {
            let mut call_nodes = Vec::new();
            find_all_nodes(root, "call_expression", &mut call_nodes);

            for call_node in call_nodes {
                let call_line = call_node.start_position().row as u32 + 1;
                if call_line >= symbol.start_line && call_line <= symbol.end_line {
                    if let Some(func) = call_node.child(0) {
                        let callee_name = match func.kind() {
                            "identifier" => node_text(func, source).to_string(),
                            "field_expression" => {
                                // obj.method()
                                find_child(func, "field_identifier")
                                    .map(|n| node_text(n, source).to_string())
                                    .unwrap_or_default()
                            }
                            "scoped_identifier" => {
                                // Module::function()
                                node_text(func, source).to_string()
                            }
                            _ => continue,
                        };
                        if !callee_name.is_empty() {
                            calls.push(Call {
                                caller_id: symbol.id.clone(),
                                callee_name,
                                call_site_line: call_line,
                            });
                        }
                    }
                }
            }
        }

        calls
    }

    fn extract_inheritance(
        &self,
        tree: &Tree,
        source: &[u8],
        symbols: &[Symbol],
    ) -> Vec<Inheritance> {
        let mut inheritance = Vec::new();
        let root = tree.root_node();

        // impl Trait for Type
        let mut impl_nodes = Vec::new();
        find_all_nodes(root, "impl_item", &mut impl_nodes);

        for node in impl_nodes {
            // Check if this is a trait impl (has "for")
            let text = node_text(node, source);
            if !text.contains(" for ") {
                continue;
            }

            // Get the type being implemented for
            let mut cursor = node.walk();
            let mut type_name = None;
            let mut trait_name = None;

            for child in node.children(&mut cursor) {
                if child.kind() == "type_identifier" || child.kind() == "generic_type" {
                    if trait_name.is_none() {
                        trait_name = Some(node_text(child, source).to_string());
                    } else {
                        type_name = Some(node_text(child, source).to_string());
                    }
                }
            }

            if let (Some(type_name), Some(trait_name)) = (type_name, trait_name) {
                if let Some(symbol) = symbols.iter().find(|s| s.name == type_name) {
                    inheritance.push(Inheritance {
                        child_id: symbol.id.clone(),
                        parent_name: trait_name,
                    });
                }
            }
        }

        inheritance
    }
}

impl RustExtractor {
    fn extract_function(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
        _parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        // Check for pub visibility
        let visibility = if node_text(node, source).trim_start().starts_with("pub") {
            Visibility::Public
        } else {
            Visibility::Private
        };

        // Check if async
        let is_async = node_text(node, source).contains("async fn");

        // Extract signature (up to opening brace)
        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .map(|s| {
                if let Some(idx) = s.find('{') {
                    s[..idx].trim().to_string()
                } else {
                    s.lines().next().unwrap_or("").to_string()
                }
            });

        // Extract doc comment
        let docstring = extract_docstring(node, source, &["line_comment", "block_comment"])
            .map(|s| s.trim_start_matches("///").trim_start_matches("//!").trim().to_string());

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Function,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async,
            docstring,
            summary: None,
            language: Language::Rust,
            parent_id: None,
        })
    }

    fn extract_struct(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "type_identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let visibility = if node_text(node, source).trim_start().starts_with("pub") {
            Visibility::Public
        } else {
            Visibility::Private
        };

        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.to_string());

        let docstring = extract_docstring(node, source, &["line_comment", "block_comment"])
            .map(|s| s.trim_start_matches("///").trim().to_string());

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Struct,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Rust,
            parent_id: None,
        })
    }

    fn extract_enum(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "type_identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let visibility = if node_text(node, source).trim_start().starts_with("pub") {
            Visibility::Public
        } else {
            Visibility::Private
        };

        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.to_string());

        let docstring = extract_docstring(node, source, &["line_comment", "block_comment"])
            .map(|s| s.trim_start_matches("///").trim().to_string());

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Enum,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Rust,
            parent_id: None,
        })
    }

    fn extract_trait(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "type_identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let visibility = if node_text(node, source).trim_start().starts_with("pub") {
            Visibility::Public
        } else {
            Visibility::Private
        };

        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.to_string());

        let docstring = extract_docstring(node, source, &["line_comment", "block_comment"])
            .map(|s| s.trim_start_matches("///").trim().to_string());

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Trait,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Rust,
            parent_id: None,
        })
    }

    fn parse_use_clause(&self, node: tree_sitter::Node, source: &[u8]) -> (String, Vec<String>) {
        match node.kind() {
            "scoped_identifier" | "identifier" => {
                (node_text(node, source).to_string(), Vec::new())
            }
            "use_as_clause" => {
                let path = node.child(0).map(|n| node_text(n, source)).unwrap_or("");
                (path.to_string(), Vec::new())
            }
            "scoped_use_list" => {
                let path = find_child(node, "scoped_identifier")
                    .or_else(|| find_child(node, "identifier"))
                    .map(|n| node_text(n, source))
                    .unwrap_or("");

                let items = find_child(node, "use_list")
                    .map(|list| {
                        let mut cursor = list.walk();
                        list.children(&mut cursor)
                            .filter(|c| c.kind() == "identifier" || c.kind() == "scoped_identifier")
                            .map(|n| node_text(n, source).to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                (path.to_string(), items)
            }
            "use_wildcard" => {
                let path = node.child(0).map(|n| node_text(n, source)).unwrap_or("");
                (format!("{}::*", path), Vec::new())
            }
            _ => (node_text(node, source).to_string(), Vec::new()),
        }
    }
}
