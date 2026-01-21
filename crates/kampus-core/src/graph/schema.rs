//! FalkorDB schema definitions
//!
//! Defines the graph schema for code symbols and relationships.

use super::{GraphError, GraphResult};
use falkordb::{AsyncGraph, FalkorClientBuilder, FalkorConnectionInfo, FalkorValue};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Graph schema for the code index
pub struct GraphSchema {
    graph: Arc<Mutex<AsyncGraph>>,
    graph_name: String,
}

impl GraphSchema {
    /// Connect to FalkorDB and create/get the graph
    pub async fn connect(
        connection_uri: Option<&str>,
        graph_name: &str,
    ) -> GraphResult<Self> {
        let uri = connection_uri.unwrap_or("redis://localhost:6379");

        let connection_info: FalkorConnectionInfo = uri
            .try_into()
            .map_err(|e: falkordb::FalkorDBError| GraphError::Connection(e.to_string()))?;

        let client = FalkorClientBuilder::new_async()
            .with_connection_info(connection_info)
            .build()
            .await
            .map_err(|e| GraphError::Connection(e.to_string()))?;

        let graph = client.select_graph(graph_name);

        Ok(Self {
            graph: Arc::new(Mutex::new(graph)),
            graph_name: graph_name.to_string(),
        })
    }

    /// Initialize the schema (create indexes, constraints)
    pub async fn initialize(&self) -> GraphResult<()> {
        let mut graph = self.graph.lock().await;

        // Create indexes for efficient lookups
        // FalkorDB syntax: CREATE INDEX FOR (n:Label) ON (n.property)
        // Note: FalkorDB will error if index already exists, so we ignore those errors
        let index_queries = [
            // Node indexes
            "CREATE INDEX FOR (f:File) ON (f.path)",
            "CREATE INDEX FOR (fn:Function) ON (fn.name)",
            "CREATE INDEX FOR (fn:Function) ON (fn.file_path)",
            "CREATE INDEX FOR (c:Class) ON (c.name)",
            "CREATE INDEX FOR (c:Class) ON (c.file_path)",
            "CREATE INDEX FOR (s:Struct) ON (s.name)",
            "CREATE INDEX FOR (s:Struct) ON (s.file_path)",
            "CREATE INDEX FOR (i:Interface) ON (i.name)",
            "CREATE INDEX FOR (m:Module) ON (m.name)",
        ];

        for query in &index_queries {
            // Ignore errors from indexes that already exist
            let _ = graph.query(query).execute().await;
        }

        Ok(())
    }

    /// Drop all data from the graph
    pub async fn clear(&self) -> GraphResult<()> {
        let mut graph = self.graph.lock().await;
        graph
            .query("MATCH (n) DETACH DELETE n")
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;
        Ok(())
    }

    /// Delete all data for a specific file
    pub async fn delete_file(&self, file_path: &str) -> GraphResult<()> {
        let mut graph = self.graph.lock().await;
        let escaped_path = escape_string(file_path);

        // Delete the file node and all symbols from that file
        let query = format!(
            r#"
            MATCH (f:File {{path: '{}'}})
            OPTIONAL MATCH (f)-[:CONTAINS]->(s)
            DETACH DELETE f, s
            "#,
            escaped_path
        );

        graph
            .query(&query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        // Also delete any orphaned symbols from this file
        let cleanup_query = format!(
            r#"
            MATCH (s)
            WHERE s.file_path = '{}'
            DETACH DELETE s
            "#,
            escaped_path
        );

        graph
            .query(&cleanup_query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        Ok(())
    }

    /// Get graph statistics
    pub async fn stats(&self) -> GraphResult<GraphStats> {
        let mut graph = self.graph.lock().await;

        // Helper to extract count from a query result
        async fn get_count(graph: &mut AsyncGraph, query: &str) -> i64 {
            match graph.query(query).execute().await {
                Ok(result) => {
                    result.data.into_iter().next()
                        .and_then(|row| row.into_iter().next())
                        .and_then(|val| match val {
                            FalkorValue::I64(n) => Some(n),
                            _ => None,
                        })
                        .unwrap_or(0)
                }
                Err(_) => 0,
            }
        }

        let file_count = get_count(&mut graph, "MATCH (f:File) RETURN count(f)").await;
        let function_count = get_count(&mut graph, "MATCH (f:Function) RETURN count(f)").await;
        let class_count = get_count(&mut graph, "MATCH (c:Class) RETURN count(c)").await;
        let struct_count = get_count(&mut graph, "MATCH (s:Struct) RETURN count(s)").await;
        let calls_count = get_count(&mut graph, "MATCH ()-[r:CALLS]->() RETURN count(r)").await;
        let imports_count = get_count(&mut graph, "MATCH ()-[r:IMPORTS]->() RETURN count(r)").await;

        Ok(GraphStats {
            file_count: file_count as usize,
            function_count: function_count as usize,
            class_count: class_count as usize,
            struct_count: struct_count as usize,
            calls_count: calls_count as usize,
            imports_count: imports_count as usize,
        })
    }

    /// Get metadata (last indexed commit, etc.)
    pub async fn get_metadata(&self, key: &str) -> GraphResult<Option<String>> {
        let mut graph = self.graph.lock().await;

        let query = format!(
            "MATCH (m:Metadata {{key: '{}'}}) RETURN m.value",
            escape_string(key)
        );

        let result = graph
            .query(&query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        Ok(result.data.into_iter().next()
            .and_then(|row| row.into_iter().next())
            .and_then(|val| match val {
                FalkorValue::String(s) => Some(s),
                _ => None,
            }))
    }

    /// Set metadata
    pub async fn set_metadata(&self, key: &str, value: &str) -> GraphResult<()> {
        let mut graph = self.graph.lock().await;

        let query = format!(
            r#"
            MERGE (m:Metadata {{key: '{}'}})
            SET m.value = '{}'
            "#,
            escape_string(key),
            escape_string(value)
        );

        graph
            .query(&query)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        Ok(())
    }

    /// Execute a raw Cypher query
    pub async fn query(&self, cypher: &str) -> GraphResult<Vec<Vec<FalkorValue>>> {
        let mut graph = self.graph.lock().await;

        let result = graph
            .query(cypher)
            .execute()
            .await
            .map_err(|e| GraphError::Query(e.to_string()))?;

        // Collect results into a Vec
        let rows: Vec<Vec<FalkorValue>> = result.data.collect();
        Ok(rows)
    }

    /// Get the underlying graph handle for batch operations
    pub fn graph(&self) -> Arc<Mutex<AsyncGraph>> {
        self.graph.clone()
    }

    /// Get the graph name
    pub fn name(&self) -> &str {
        &self.graph_name
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

/// Graph statistics
#[derive(Debug, Clone, Default)]
pub struct GraphStats {
    pub file_count: usize,
    pub function_count: usize,
    pub class_count: usize,
    pub struct_count: usize,
    pub calls_count: usize,
    pub imports_count: usize,
}

impl std::fmt::Display for GraphStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Graph Statistics:")?;
        writeln!(f, "  Files:     {}", self.file_count)?;
        writeln!(f, "  Functions: {}", self.function_count)?;
        writeln!(f, "  Classes:   {}", self.class_count)?;
        writeln!(f, "  Structs:   {}", self.struct_count)?;
        writeln!(f, "  Calls:     {}", self.calls_count)?;
        writeln!(f, "  Imports:   {}", self.imports_count)?;
        Ok(())
    }
}
