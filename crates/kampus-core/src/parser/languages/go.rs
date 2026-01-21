//! Go language extractor

use crate::parser::extractor::{
    extract_docstring, find_all_nodes, find_child, node_text, LanguageExtractor,
};
use crate::{Call, Import, Inheritance, Language, Symbol, SymbolKind, Visibility};
use std::path::Path;
use tree_sitter::Tree;

pub struct GoExtractor;

impl LanguageExtractor for GoExtractor {
    fn extract_symbols(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let root = tree.root_node();

        // Function declarations
        let mut func_nodes = Vec::new();
        find_all_nodes(root, "function_declaration", &mut func_nodes);
        for node in func_nodes {
            if let Some(symbol) = self.extract_function(node, source, file_path) {
                symbols.push(symbol);
            }
        }

        // Method declarations (receiver functions)
        let mut method_nodes = Vec::new();
        find_all_nodes(root, "method_declaration", &mut method_nodes);
        for node in method_nodes {
            if let Some(symbol) = self.extract_method(node, source, file_path, &symbols) {
                symbols.push(symbol);
            }
        }

        // Type declarations (structs, interfaces)
        let mut type_nodes = Vec::new();
        find_all_nodes(root, "type_declaration", &mut type_nodes);
        for node in type_nodes {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec" {
                    if let Some(symbol) = self.extract_type_spec(child, source, file_path) {
                        symbols.push(symbol);
                    }
                }
            }
        }

        symbols
    }

    fn extract_imports(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Import> {
        let mut imports = Vec::new();
        let root = tree.root_node();

        // import declarations
        let mut import_nodes = Vec::new();
        find_all_nodes(root, "import_declaration", &mut import_nodes);

        for node in import_nodes {
            // Single import: import "fmt"
            if let Some(path) = find_child(node, "interpreted_string_literal") {
                let target = node_text(path, source)
                    .trim_matches('"')
                    .to_string();
                imports.push(Import {
                    source_file: file_path.to_path_buf(),
                    target,
                    alias: None,
                    items: Vec::new(),
                    line: node.start_position().row as u32 + 1,
                });
            }

            // Import list: import ( "fmt" ; "os" )
            if let Some(spec_list) = find_child(node, "import_spec_list") {
                let mut cursor = spec_list.walk();
                for child in spec_list.children(&mut cursor) {
                    if child.kind() == "import_spec" {
                        if let Some(path) = find_child(child, "interpreted_string_literal") {
                            let target = node_text(path, source)
                                .trim_matches('"')
                                .to_string();

                            let alias = find_child(child, "package_identifier")
                                .or_else(|| find_child(child, "dot"))
                                .or_else(|| find_child(child, "blank_identifier"))
                                .map(|n| node_text(n, source).to_string());

                            imports.push(Import {
                                source_file: file_path.to_path_buf(),
                                target,
                                alias,
                                items: Vec::new(),
                                line: child.start_position().row as u32 + 1,
                            });
                        }
                    }
                }
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
                            "selector_expression" => {
                                // obj.Method() or pkg.Function()
                                find_child(func, "field_identifier")
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

        // Look for struct embedding
        let mut type_nodes = Vec::new();
        find_all_nodes(root, "type_spec", &mut type_nodes);

        for node in type_nodes {
            let type_name = find_child(node, "type_identifier")
                .map(|n| node_text(n, source))
                .unwrap_or("");

            // Check for struct type with embedded fields
            if let Some(struct_type) = find_child(node, "struct_type") {
                if let Some(field_list) = find_child(struct_type, "field_declaration_list") {
                    let mut cursor = field_list.walk();
                    for field in field_list.children(&mut cursor) {
                        if field.kind() == "field_declaration" {
                            // Embedded field has no name, just a type
                            let has_name = find_child(field, "field_identifier").is_some();
                            if !has_name {
                                if let Some(embedded_type) = find_child(field, "type_identifier") {
                                    if let Some(symbol) = symbols
                                        .iter()
                                        .find(|s| s.kind == SymbolKind::Struct && s.name == type_name)
                                    {
                                        inheritance.push(Inheritance {
                                            child_id: symbol.id.clone(),
                                            parent_name: node_text(embedded_type, source).to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check for interface embedding
            if let Some(interface_type) = find_child(node, "interface_type") {
                let mut cursor = interface_type.walk();
                for child in interface_type.children(&mut cursor) {
                    if child.kind() == "type_identifier" {
                        if let Some(symbol) = symbols
                            .iter()
                            .find(|s| s.kind == SymbolKind::Interface && s.name == type_name)
                        {
                            inheritance.push(Inheritance {
                                child_id: symbol.id.clone(),
                                parent_name: node_text(child, source).to_string(),
                            });
                        }
                    }
                }
            }
        }

        inheritance
    }
}

impl GoExtractor {
    fn extract_function(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        // Go visibility: uppercase = public, lowercase = private
        let visibility = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            Visibility::Public
        } else {
            Visibility::Private
        };

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

        let docstring = extract_docstring(node, source, &["comment"]);

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Function,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async: false, // Go uses goroutines, not async/await
            docstring,
            summary: None,
            language: Language::Go,
            parent_id: None,
        })
    }

    fn extract_method(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
        existing_symbols: &[Symbol],
    ) -> Option<Symbol> {
        let name_node = find_child(node, "field_identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let visibility = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            Visibility::Public
        } else {
            Visibility::Private
        };

        // Get receiver type
        let receiver_type = find_child(node, "parameter_list")
            .and_then(|params| {
                let mut cursor = params.walk();
                params.children(&mut cursor).find(|c| c.kind() == "parameter_declaration")
            })
            .and_then(|param| {
                find_child(param, "type_identifier")
                    .or_else(|| find_child(param, "pointer_type"))
            })
            .map(|n| {
                let text = node_text(n, source);
                text.trim_start_matches('*').to_string()
            });

        let parent_id = receiver_type.as_ref().and_then(|rt| {
            existing_symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Struct && s.name == *rt)
                .map(|s| s.id.clone())
        });

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

        let docstring = extract_docstring(node, source, &["comment"]);

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Method,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Go,
            parent_id,
        })
    }

    fn extract_type_spec(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "type_identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let visibility = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            Visibility::Public
        } else {
            Visibility::Private
        };

        // Determine if struct or interface
        let kind = if find_child(node, "struct_type").is_some() {
            SymbolKind::Struct
        } else if find_child(node, "interface_type").is_some() {
            SymbolKind::Interface
        } else {
            // Type alias
            SymbolKind::Interface
        };

        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.to_string());

        let docstring = node
            .prev_sibling()
            .filter(|n| n.kind() == "comment")
            .map(|n| node_text(n, source).to_string());

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Go,
            parent_id: None,
        })
    }
}
