//! Python language extractor

use crate::parser::extractor::{
    find_all_nodes, find_child, find_children, node_text, LanguageExtractor,
};
use crate::{Call, Import, Inheritance, Language, Symbol, SymbolKind, Visibility};
use std::path::Path;
use tree_sitter::Tree;

pub struct PythonExtractor;

impl LanguageExtractor for PythonExtractor {
    fn extract_symbols(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let root = tree.root_node();

        // Extract top-level functions
        let mut func_nodes = Vec::new();
        find_all_nodes(root, "function_definition", &mut func_nodes);
        for node in func_nodes {
            if let Some(symbol) = self.extract_function(node, source, file_path, None) {
                symbols.push(symbol);
            }
        }

        // Extract classes and their methods
        let mut class_nodes = Vec::new();
        find_all_nodes(root, "class_definition", &mut class_nodes);
        for node in class_nodes {
            if let Some(class_symbol) = self.extract_class(node, source, file_path) {
                let class_id = class_symbol.id.clone();
                symbols.push(class_symbol);

                // Extract methods from class body
                if let Some(body) = find_child(node, "block") {
                    for child in find_children(body, "function_definition") {
                        if let Some(method) =
                            self.extract_function(child, source, file_path, Some(&class_id))
                        {
                            symbols.push(method);
                        }
                    }
                }
            }
        }

        symbols
    }

    fn extract_imports(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Import> {
        let mut imports = Vec::new();
        let root = tree.root_node();

        // import x, y
        let mut import_nodes = Vec::new();
        find_all_nodes(root, "import_statement", &mut import_nodes);
        for node in import_nodes {
            for name_node in find_children(node, "dotted_name") {
                let target = node_text(name_node, source).to_string();
                let alias = find_child(node, "aliased_import")
                    .and_then(|n| find_child(n, "identifier"))
                    .map(|n| node_text(n, source).to_string());
                imports.push(Import {
                    source_file: file_path.to_path_buf(),
                    target,
                    alias,
                    items: Vec::new(),
                    line: node.start_position().row as u32 + 1,
                });
            }
        }

        // from x import y, z
        let mut from_nodes = Vec::new();
        find_all_nodes(root, "import_from_statement", &mut from_nodes);
        for node in from_nodes {
            let target = find_child(node, "dotted_name")
                .or_else(|| find_child(node, "relative_import"))
                .map(|n| node_text(n, source).to_string())
                .unwrap_or_default();

            let items: Vec<String> = find_children(node, "dotted_name")
                .into_iter()
                .skip(1) // Skip the module name
                .map(|n| node_text(n, source).to_string())
                .collect();

            imports.push(Import {
                source_file: file_path.to_path_buf(),
                target,
                alias: None,
                items,
                line: node.start_position().row as u32 + 1,
            });
        }

        imports
    }

    fn extract_calls(&self, tree: &Tree, source: &[u8], symbols: &[Symbol]) -> Vec<Call> {
        let mut calls = Vec::new();
        let root = tree.root_node();

        // Build a map of function bodies
        for symbol in symbols.iter().filter(|s| {
            s.kind == SymbolKind::Function || s.kind == SymbolKind::Method
        }) {
            let mut call_nodes = Vec::new();
            find_all_nodes(root, "call", &mut call_nodes);

            for call_node in call_nodes {
                let call_line = call_node.start_position().row as u32 + 1;
                if call_line >= symbol.start_line && call_line <= symbol.end_line {
                    if let Some(func) = call_node.child(0) {
                        let callee_name = match func.kind() {
                            "identifier" => node_text(func, source).to_string(),
                            "attribute" => {
                                // obj.method() - get the method name
                                find_child(func, "identifier")
                                    .map(|n| node_text(n, source).to_string())
                                    .unwrap_or_default()
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

        let mut class_nodes = Vec::new();
        find_all_nodes(root, "class_definition", &mut class_nodes);

        for node in class_nodes {
            let class_name = find_child(node, "identifier")
                .map(|n| node_text(n, source))
                .unwrap_or("");

            // Find matching symbol
            let class_symbol = symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Class && s.name == class_name);

            if let Some(symbol) = class_symbol {
                // Look for argument_list (base classes)
                if let Some(bases) = find_child(node, "argument_list") {
                    let mut cursor = bases.walk();
                    for base in bases.children(&mut cursor) {
                        if base.kind() == "identifier" || base.kind() == "attribute" {
                            let parent_name = node_text(base, source).to_string();
                            inheritance.push(Inheritance {
                                child_id: symbol.id.clone(),
                                parent_name,
                            });
                        }
                    }
                }
            }
        }

        inheritance
    }
}

impl PythonExtractor {
    fn extract_function(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let kind = if parent_id.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        // Check if async
        let is_async = node
            .prev_sibling()
            .map(|n| n.kind() == "async")
            .unwrap_or(false)
            || node.parent().map(|p| p.kind() == "async_function_definition").unwrap_or(false);

        // Extract signature (first line)
        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.trim_end_matches(':').to_string());

        // Extract docstring (first string in body)
        let docstring = find_child(node, "block")
            .and_then(|body| {
                let mut cursor = body.walk();
                body.children(&mut cursor)
                    .find(|c| c.kind() == "expression_statement")
            })
            .and_then(|expr| find_child(expr, "string"))
            .map(|s| {
                let text = node_text(s, source);
                // Strip triple quotes
                text.trim_start_matches("\"\"\"")
                    .trim_start_matches("'''")
                    .trim_end_matches("\"\"\"")
                    .trim_end_matches("'''")
                    .trim()
                    .to_string()
            });

        // Visibility based on name convention
        let visibility = if name.starts_with("__") && !name.ends_with("__") {
            Visibility::Private
        } else if name.starts_with('_') {
            Visibility::Protected
        } else {
            Visibility::Public
        };

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async,
            docstring,
            summary: None,
            language: Language::Python,
            parent_id: parent_id.map(String::from),
        })
    }

    fn extract_class(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        // Extract signature (class declaration line)
        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.trim_end_matches(':').to_string());

        // Extract docstring
        let docstring = find_child(node, "block")
            .and_then(|body| {
                let mut cursor = body.walk();
                body.children(&mut cursor)
                    .find(|c| c.kind() == "expression_statement")
            })
            .and_then(|expr| find_child(expr, "string"))
            .map(|s| {
                let text = node_text(s, source);
                text.trim_start_matches("\"\"\"")
                    .trim_start_matches("'''")
                    .trim_end_matches("\"\"\"")
                    .trim_end_matches("'''")
                    .trim()
                    .to_string()
            });

        let visibility = if name.starts_with('_') {
            Visibility::Private
        } else {
            Visibility::Public
        };

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Class,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Python,
            parent_id: None,
        })
    }
}
