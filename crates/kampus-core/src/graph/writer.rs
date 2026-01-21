//! Batch writer for graph database operations

use super::{GraphError, GraphResult, GraphSchema};
use crate::{Call, FileSymbols, Import, Inheritance, Symbol, SymbolKind};

/// Batch size for database writes
const BATCH_SIZE: usize = 1000;

/// Writes symbols and relationships to the graph database in batches
pub struct GraphWriter {
    schema: GraphSchema,
}

impl GraphWriter {
    pub fn new(schema: GraphSchema) -> Self {
        Self { schema }
    }

    /// Write a batch of file symbols to the database
    pub async fn write_files(&self, files: Vec<FileSymbols>) -> GraphResult<WriteStats> {
        let mut stats = WriteStats::default();

        // Process in batches
        for chunk in files.chunks(BATCH_SIZE) {
            self.write_file_batch(chunk, &mut stats).await?;
        }

        Ok(stats)
    }

    /// Write a single file's symbols
    pub async fn write_file(&self, file_symbols: &FileSymbols) -> GraphResult<()> {
        let graph = self.schema.graph();
        let mut graph = graph.lock().await;

        let file_path = file_symbols.file_path.to_string_lossy().to_string();
        let language = file_symbols
            .language
            .map(|l| l.to_string())
            .unwrap_or_default();

        // Create file node - embed values directly in query
        let file_query = format!(
            r#"
            MERGE (f:File {{path: '{}'}})
            SET f.language = '{}',
                f.hash = '{}',
                f.line_count = {},
                f.last_indexed = timestamp()
            "#,
            escape_string(&file_path),
            escape_string(&language),
            escape_string(&file_symbols.content_hash),
            file_symbols.line_count
        );

        graph
            .query(&file_query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        // Create symbol nodes
        for symbol in &file_symbols.symbols {
            self.write_symbol(&mut graph, symbol, &file_path).await?;
        }

        // Create import relationships
        for import in &file_symbols.imports {
            self.write_import(&mut graph, import).await?;
        }

        // Create call relationships
        for call in &file_symbols.calls {
            self.write_call(&mut graph, call).await?;
        }

        // Create inheritance relationships
        for inheritance in &file_symbols.inheritance {
            self.write_inheritance(&mut graph, inheritance).await?;
        }

        Ok(())
    }

    async fn write_file_batch(
        &self,
        files: &[FileSymbols],
        stats: &mut WriteStats,
    ) -> GraphResult<()> {
        for file_symbols in files {
            self.write_file(file_symbols).await?;
            stats.files_written += 1;
            stats.symbols_written += file_symbols.symbols.len();
            stats.imports_written += file_symbols.imports.len();
            stats.calls_written += file_symbols.calls.len();
        }
        Ok(())
    }

    async fn write_symbol(
        &self,
        graph: &mut falkordb::AsyncGraph,
        symbol: &Symbol,
        file_path: &str,
    ) -> GraphResult<()> {
        let label = symbol_kind_to_label(symbol.kind);
        let visibility = format!("{:?}", symbol.visibility).to_lowercase();

        let query = format!(
            r#"
            MERGE (s:{label} {{id: '{id}'}})
            SET s.name = '{name}',
                s.file_path = '{file_path}',
                s.start_line = {start_line},
                s.end_line = {end_line},
                s.signature = '{signature}',
                s.visibility = '{visibility}',
                s.is_async = {is_async},
                s.docstring = '{docstring}',
                s.language = '{language}'
            WITH s
            MATCH (f:File {{path: '{file_path}'}})
            MERGE (f)-[:CONTAINS]->(s)
            "#,
            label = label,
            id = escape_string(&symbol.id),
            name = escape_string(&symbol.name),
            file_path = escape_string(file_path),
            start_line = symbol.start_line,
            end_line = symbol.end_line,
            signature = escape_string(symbol.signature.as_deref().unwrap_or("")),
            visibility = escape_string(&visibility),
            is_async = symbol.is_async,
            docstring = escape_string(symbol.docstring.as_deref().unwrap_or("")),
            language = symbol.language.to_string()
        );

        graph
            .query(&query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        // If this symbol has a parent, create the CONTAINS relationship
        if let Some(ref parent_id) = symbol.parent_id {
            let parent_query = format!(
                r#"
                MATCH (p {{id: '{}'}})
                MATCH (c {{id: '{}'}})
                MERGE (p)-[:CONTAINS]->(c)
                "#,
                escape_string(parent_id),
                escape_string(&symbol.id)
            );

            graph
                .query(&parent_query)
                .execute()
                .await
                .map_err(|e| GraphError::Query(e.to_string()))?;
        }

        Ok(())
    }

    async fn write_import(
        &self,
        graph: &mut falkordb::AsyncGraph,
        import: &Import,
    ) -> GraphResult<()> {
        let source_path = import.source_file.to_string_lossy();
        let items_json = serde_json::to_string(&import.items).unwrap_or_default();

        // Create a Module node for the import target and link to it
        let query = format!(
            r#"
            MERGE (m:Module {{name: '{}'}})
            SET m.is_external = true
            WITH m
            MATCH (f:File {{path: '{}'}})
            MERGE (f)-[r:IMPORTS]->(m)
            SET r.alias = '{}',
                r.items = '{}',
                r.line = {}
            "#,
            escape_string(&import.target),
            escape_string(&source_path),
            escape_string(import.alias.as_deref().unwrap_or("")),
            escape_string(&items_json),
            import.line
        );

        graph
            .query(&query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        Ok(())
    }

    async fn write_call(
        &self,
        graph: &mut falkordb::AsyncGraph,
        call: &Call,
    ) -> GraphResult<()> {
        // Try to link to an existing function by name
        let query = format!(
            r#"
            MATCH (caller {{id: '{}'}})
            OPTIONAL MATCH (callee:Function {{name: '{}'}})
            FOREACH (_ IN CASE WHEN callee IS NOT NULL THEN [1] ELSE [] END |
                MERGE (caller)-[r:CALLS]->(callee)
                SET r.call_site_line = {}
            )
            "#,
            escape_string(&call.caller_id),
            escape_string(&call.callee_name),
            call.call_site_line
        );

        graph
            .query(&query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        Ok(())
    }

    async fn write_inheritance(
        &self,
        graph: &mut falkordb::AsyncGraph,
        inheritance: &Inheritance,
    ) -> GraphResult<()> {
        // Create INHERITS relationship
        let query = format!(
            r#"
            MATCH (child {{id: '{}'}})
            OPTIONAL MATCH (parent)
            WHERE (parent:Class OR parent:Struct OR parent:Interface OR parent:Trait)
              AND parent.name = '{}'
            FOREACH (_ IN CASE WHEN parent IS NOT NULL THEN [1] ELSE [] END |
                MERGE (child)-[:INHERITS]->(parent)
            )
            "#,
            escape_string(&inheritance.child_id),
            escape_string(&inheritance.parent_name)
        );

        graph
            .query(&query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        Ok(())
    }

    /// Get the schema for direct operations
    pub fn schema(&self) -> &GraphSchema {
        &self.schema
    }
}

/// Escape a string for use in a Cypher query
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Statistics from write operations
#[derive(Debug, Clone, Default)]
pub struct WriteStats {
    pub files_written: usize,
    pub symbols_written: usize,
    pub imports_written: usize,
    pub calls_written: usize,
}

impl std::fmt::Display for WriteStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Write Statistics:")?;
        writeln!(f, "  Files:   {}", self.files_written)?;
        writeln!(f, "  Symbols: {}", self.symbols_written)?;
        writeln!(f, "  Imports: {}", self.imports_written)?;
        writeln!(f, "  Calls:   {}", self.calls_written)?;
        Ok(())
    }
}

/// Convert SymbolKind to graph node label
fn symbol_kind_to_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "Function",
        SymbolKind::Class => "Class",
        SymbolKind::Struct => "Struct",
        SymbolKind::Interface => "Interface",
        SymbolKind::Module => "Module",
        SymbolKind::Method => "Method",
        SymbolKind::Trait => "Trait",
        SymbolKind::Enum => "Enum",
        SymbolKind::Constant => "Constant",
        SymbolKind::Variable => "Variable",
    }
}
