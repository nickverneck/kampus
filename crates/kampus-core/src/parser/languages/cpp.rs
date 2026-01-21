//! C++ language extractor

use crate::parser::extractor::{
    extract_docstring, find_all_nodes, find_child, node_text, LanguageExtractor,
};
use crate::{Call, Import, Inheritance, Language, Symbol, SymbolKind, Visibility};
use std::path::Path;
use tree_sitter::Tree;

pub struct CppExtractor;

impl LanguageExtractor for CppExtractor {
    fn extract_symbols(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let root = tree.root_node();

        // Function definitions
        let mut func_nodes = Vec::new();
        find_all_nodes(root, "function_definition", &mut func_nodes);
        for node in func_nodes {
            if let Some(symbol) = self.extract_function(node, source, file_path, None) {
                symbols.push(symbol);
            }
        }

        // Class/struct definitions
        let mut class_nodes = Vec::new();
        find_all_nodes(root, "class_specifier", &mut class_nodes);
        find_all_nodes(root, "struct_specifier", &mut class_nodes);
        for node in class_nodes {
            if let Some(class_symbol) = self.extract_class(node, source, file_path) {
                let class_id = class_symbol.id.clone();
                let is_struct = node.kind() == "struct_specifier";
                symbols.push(class_symbol);

                // Extract methods from class body
                if let Some(body) = find_child(node, "field_declaration_list") {
                    // Track current access specifier
                    let mut current_visibility = if is_struct {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    };

                    let mut cursor = body.walk();
                    for child in body.children(&mut cursor) {
                        match child.kind() {
                            "access_specifier" => {
                                let text = node_text(child, source).to_lowercase();
                                current_visibility = if text.contains("public") {
                                    Visibility::Public
                                } else if text.contains("protected") {
                                    Visibility::Protected
                                } else {
                                    Visibility::Private
                                };
                            }
                            "function_definition" => {
                                if let Some(mut method) =
                                    self.extract_function(child, source, file_path, Some(&class_id))
                                {
                                    method.visibility = current_visibility;
                                    method.kind = SymbolKind::Method;
                                    symbols.push(method);
                                }
                            }
                            "declaration" => {
                                // Method declaration (not definition)
                                if let Some(declarator) = find_child(child, "function_declarator") {
                                    if let Some(mut method) = self.extract_method_declaration(
                                        child,
                                        declarator,
                                        source,
                                        file_path,
                                        &class_id,
                                    ) {
                                        method.visibility = current_visibility;
                                        symbols.push(method);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Enum definitions
        let mut enum_nodes = Vec::new();
        find_all_nodes(root, "enum_specifier", &mut enum_nodes);
        for node in enum_nodes {
            if let Some(symbol) = self.extract_enum(node, source, file_path) {
                symbols.push(symbol);
            }
        }

        symbols
    }

    fn extract_imports(&self, tree: &Tree, source: &[u8], file_path: &Path) -> Vec<Import> {
        let mut imports = Vec::new();
        let root = tree.root_node();

        // #include directives
        let mut include_nodes = Vec::new();
        find_all_nodes(root, "preproc_include", &mut include_nodes);

        for node in include_nodes {
            let target = find_child(node, "string_literal")
                .or_else(|| find_child(node, "system_lib_string"))
                .map(|n| {
                    node_text(n, source)
                        .trim_matches('"')
                        .trim_matches('<')
                        .trim_matches('>')
                        .to_string()
                })
                .unwrap_or_default();

            if !target.is_empty() {
                imports.push(Import {
                    source_file: file_path.to_path_buf(),
                    target,
                    alias: None,
                    items: Vec::new(),
                    line: node.start_position().row as u32 + 1,
                });
            }
        }

        // using declarations
        let mut using_nodes = Vec::new();
        find_all_nodes(root, "using_declaration", &mut using_nodes);
        for node in using_nodes {
            let target = node_text(node, source)
                .trim_start_matches("using")
                .trim()
                .trim_end_matches(';')
                .to_string();

            if !target.is_empty() {
                imports.push(Import {
                    source_file: file_path.to_path_buf(),
                    target,
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
                                // obj.method() or obj->method()
                                find_child(func, "field_identifier")
                                    .map(|n| node_text(n, source).to_string())
                                    .unwrap_or_default()
                            }
                            "qualified_identifier" => {
                                // Namespace::function()
                                node_text(func, source).to_string()
                            }
                            "template_function" => {
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
        find_all_nodes(root, "class_specifier", &mut class_nodes);
        find_all_nodes(root, "struct_specifier", &mut class_nodes);

        for node in class_nodes {
            let class_name = find_child(node, "type_identifier")
                .map(|n| node_text(n, source))
                .unwrap_or("");

            // Look for base_class_clause
            if let Some(base_clause) = find_child(node, "base_class_clause") {
                let mut cursor = base_clause.walk();
                for child in base_clause.children(&mut cursor) {
                    if child.kind() == "type_identifier" || child.kind() == "qualified_identifier" {
                        let parent_name = node_text(child, source).to_string();
                        if let Some(symbol) = symbols.iter().find(|s| {
                            (s.kind == SymbolKind::Class || s.kind == SymbolKind::Struct)
                                && s.name == class_name
                        }) {
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

impl CppExtractor {
    fn extract_function(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        // Get the declarator which contains the function name
        let declarator = find_child(node, "function_declarator")
            .or_else(|| find_child(node, "pointer_declarator"))?;

        let name = self.extract_function_name(declarator, source)?;

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

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

        let kind = if parent_id.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility: Visibility::Public,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Cpp,
            parent_id: parent_id.map(String::from),
        })
    }

    fn extract_function_name(&self, declarator: tree_sitter::Node, source: &[u8]) -> Option<String> {
        // Direct identifier
        if let Some(id) = find_child(declarator, "identifier") {
            return Some(node_text(id, source).to_string());
        }

        // Qualified identifier (Class::method)
        if let Some(qualified) = find_child(declarator, "qualified_identifier") {
            return Some(node_text(qualified, source).to_string());
        }

        // Destructor
        if let Some(destructor) = find_child(declarator, "destructor_name") {
            return Some(node_text(destructor, source).to_string());
        }

        // Nested declarator
        if let Some(nested) = find_child(declarator, "function_declarator") {
            return self.extract_function_name(nested, source);
        }

        None
    }

    fn extract_method_declaration(
        &self,
        _declaration: tree_sitter::Node,
        declarator: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
        parent_id: &str,
    ) -> Option<Symbol> {
        let name = self.extract_function_name(declarator, source)?;

        let start_line = declarator.start_position().row as u32 + 1;
        let end_line = declarator.end_position().row as u32 + 1;

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Method,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature: None,
            visibility: Visibility::Private,
            is_async: false,
            docstring: None,
            summary: None,
            language: Language::Cpp,
            parent_id: Some(parent_id.to_string()),
        })
    }

    fn extract_class(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let name_node = find_child(node, "type_identifier")?;
        let name = node_text(name_node, source).to_string();

        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let kind = if node.kind() == "struct_specifier" {
            SymbolKind::Struct
        } else {
            SymbolKind::Class
        };

        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.to_string());

        let docstring = extract_docstring(node, source, &["comment"]);

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility: Visibility::Public,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Cpp,
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

        let signature = source
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.lines().next())
            .map(|s| s.to_string());

        let docstring = extract_docstring(node, source, &["comment"]);

        Some(Symbol {
            id: Symbol::generate_id(file_path, &name, start_line),
            name,
            kind: SymbolKind::Enum,
            file_path: file_path.to_path_buf(),
            start_line,
            end_line,
            signature,
            visibility: Visibility::Public,
            is_async: false,
            docstring,
            summary: None,
            language: Language::Cpp,
            parent_id: None,
        })
    }
}
