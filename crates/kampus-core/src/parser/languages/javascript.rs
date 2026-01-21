//! JavaScript language extractor

use crate::parser::extractor::{
    extract_docstring, find_all_nodes, find_child, find_children, node_text, LanguageExtractor,
};
use crate::{Call, Import, Inheritance, Language, Symbol, SymbolKind, Visibility};
use std::path::Path;
use tree_sitter::Tree;

pub struct JavaScriptExtractor;

impl LanguageExtractor for JavaScriptExtractor {
    fn extract_symbols(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let root = tree.root_node();

        // Function declarations
        let mut func_nodes = Vec::new();
        find_all_nodes(root, "function_declaration", &mut func_nodes);
        for node in func_nodes {
            if let Some(symbol) = self.extract_function(node, source, file_path, None) {
                symbols.push(symbol);
            }
        }

        // Arrow functions assigned to variables
        let mut var_nodes = Vec::new();
        find_all_nodes(root, "lexical_declaration", &mut var_nodes);
        find_all_nodes(root, "variable_declaration", &mut var_nodes);
        for node in var_nodes {
            for declarator in find_children(node, "variable_declarator") {
                if let Some(value) = find_child(declarator, "arrow_function") {
                    if let Some(name) = find_child(declarator, "identifier") {
                        if let Some(symbol) =
                            self.extract_arrow_function(value, name, source, file_path)
                        {
                            symbols.push(symbol);
                        }
                    }
                }
            }
        }

        // Classes
        let mut class_nodes = Vec::new();
        find_all_nodes(root, "class_declaration", &mut class_nodes);
        for node in class_nodes {
            if let Some(class_symbol) = self.extract_class(node, source, file_path) {
                let class_id = class_symbol.id.clone();
                symbols.push(class_symbol);

                // Extract methods
                if let Some(body) = find_child(node, "class_body") {
                    for method in find_children(body, "method_definition") {
                        if let Some(method_symbol) =
                            self.extract_method(method, source, file_path, &class_id)
                        {
                            symbols.push(method_symbol);
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

        // ES6 imports
        let mut import_nodes = Vec::new();
        find_all_nodes(root, "import_statement", &mut import_nodes);
        for node in import_nodes {
            let target = find_child(node, "string")
                .map(|n| {
                    node_text(n, source)
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string()
                })
                .unwrap_or_default();

            let mut items = Vec::new();

            // Named imports: import { a, b } from 'x'
            if let Some(clause) = find_child(node, "import_clause") {
                if let Some(named) = find_child(clause, "named_imports") {
                    let mut cursor = named.walk();
                    for specifier in named.children(&mut cursor) {
                        if specifier.kind() == "import_specifier" {
                            if let Some(name) = find_child(specifier, "identifier") {
                                items.push(node_text(name, source).to_string());
                            }
                        }
                    }
                }

                // Default import: import x from 'y'
                if let Some(default) = find_child(clause, "identifier") {
                    items.push(node_text(default, source).to_string());
                }
            }

            imports.push(Import {
                source_file: file_path.to_path_buf(),
                target,
                alias: None,
                items,
                line: node.start_position().row as u32 + 1,
            });
        }

        // CommonJS require
        let mut call_nodes = Vec::new();
        find_all_nodes(root, "call_expression", &mut call_nodes);
        for node in call_nodes {
            if let Some(func) = node.child(0) {
                if node_text(func, source) == "require" {
                    if let Some(args) = find_child(node, "arguments") {
                        if let Some(arg) = args.child(1) {
                            // Skip opening paren
                            let target = node_text(arg, source)
                                .trim_matches('"')
                                .trim_matches('\'')
                                .to_string();
                            imports.push(Import {
                                source_file: file_path.to_path_buf(),
                                target,
                                alias: None,
                                items: Vec::new(),
                                line: node.start_position().row as u32 + 1,
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
                            "member_expression" => {
                                find_child(func, "property_identifier")
                                    .map(|n| node_text(n, source).to_string())
                                    .unwrap_or_default()
                            }
                            _ => continue,
                        };
                        if !callee_name.is_empty() && callee_name != "require" {
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
        find_all_nodes(root, "class_declaration", &mut class_nodes);

        for node in class_nodes {
            let class_name = find_child(node, "identifier")
                .map(|n| node_text(n, source))
                .unwrap_or("");

            if let Some(heritage) = find_child(node, "class_heritage") {
                if let Some(extends) = find_child(heritage, "identifier") {
                    let parent_name = node_text(extends, source).to_string();
                    if let Some(symbol) = symbols
                        .iter()
                        .find(|s| s.kind == SymbolKind::Class && s.name == class_name)
                    {
                        inheritance.push(Inheritance {
                            child_id: symbol.id.clone(),
                            parent_name,
                        });
                    }
                }
            }
        }

        inheritance
    }
}

impl JavaScriptExtractor {
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

        let is_async = node_text(node, source).trim_start().starts_with("async");

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
            visibility: Visibility::Public,
            is_async,
            docstring,
            summary: None,
            language: Language::JavaScript,
            parent_id: None,
        })
    }

    fn extract_arrow_function(
        &self,
        node: tree_sitter::Node,
        name_node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let name = node_text(name_node, source).to_string();

        let start_line = name_node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let is_async = node_text(node, source).trim_start().starts_with("async");

        let signature = Some(format!(
            "const {} = {}",
            name,
            source
                .get(node.start_byte()..node.end_byte())
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .map(|s| {
                    if let Some(idx) = s.find("=>") {
                        s[..idx + 2].trim().to_string()
                    } else {
                        s.lines().next().unwrap_or("").to_string()
                    }
                })
                .unwrap_or_default()
        ));

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Function,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility: Visibility::Public,
            is_async,
            docstring: None,
            summary: None,
            language: Language::JavaScript,
            parent_id: None,
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

        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.trim_end_matches('{').trim().to_string());

        let docstring = extract_docstring(node, source, &["comment"]);

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Class,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility: Visibility::Public,
            is_async: false,
            docstring,
            summary: None,
            language: Language::JavaScript,
            parent_id: None,
        })
    }

    fn extract_method(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
        parent_id: &str,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "property_identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let is_async = node_text(node, source).trim_start().starts_with("async");

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

        let visibility = if name.starts_with('_') {
            Visibility::Private
        } else {
            Visibility::Public
        };

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Method,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility,
            is_async,
            docstring: None,
            summary: None,
            language: Language::JavaScript,
            parent_id: Some(parent_id.to_string()),
        })
    }
}
