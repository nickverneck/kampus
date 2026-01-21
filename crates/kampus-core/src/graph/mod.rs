//! Graph database operations using FalkorDB
//!
//! Provides schema definition, batch writing, and query operations.

pub mod schema;
pub mod writer;

pub use falkordb::{AsyncGraph, FalkorValue};
pub use schema::GraphSchema;
pub use writer::GraphWriter;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphError {
    #[error("Database connection error: {0}")]
    Connection(String),
    #[error("Query execution error: {0}")]
    Query(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Schema error: {0}")]
    Schema(String),
}

/// Result type for graph operations
pub type GraphResult<T> = Result<T, GraphError>;
