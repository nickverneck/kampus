# Kampus

> Long term memory for coding repositories.

Kampus is a tree-sitter based code indexing tool that creates a queryable knowledge graph of your codebase. It parses your source code to extract symbols (functions, classes, structs, etc.) and their relationships (calls, inheritance, etc.), storing them in a graph database for complex querying and analysis.

## Features

-   **Multi-language Support**: Supports C++, Go, JavaScript, TypeScript, Python, and Rust.
-   **Graph Database**: Stores data in FalkorDB (a high-performance graph database) enabling Cypher queries.
-   **Incremental Indexing**: Updates the index based on Git changes to keep it in sync.
-   **Semantic Search**: Find symbols by name, type, or relationship.
-   **Call Graphs**: Analyze function call chains (callers/callees).

## Prerequisites

-   [FalkorDB](https://falkordb.com/) (or a Redis instance with the FalkorDB module loaded).
    -   Default URI: `redis://localhost:6379`

## Installation

```bash
# Clone the repository
git clone https://github.com/nickverneck/kampus.git
cd kampus

# Build and install using Cargo
cargo install --path crates/kampus-cli
```

## Usage

Run the `kampus` command to interact with the tool.

### Global Options

-   `--verbose`, `-v`: Enable verbose logging.
-   `--db-uri`: FalkorDB connection URI (default: `redis://localhost:6379`). Can also be set via `KAMPUS_DB_URI` environment variable.
-   `--graph`: Name of the graph to use (default: `kampus`).

### Commands

#### 1. Indexing (`index`)

Index a codebase from scratch.

```bash
# Index current directory
kampus index

# Index specific path with specific languages
kampus index /path/to/repo --languages rs,py --jobs 4
```

#### 2. Updating (`update`)

Incrementally update the index based on changes in the git repository.

```bash
# Update index based on changes since last index
kampus update

# See what would change without updating
kampus update --dry-run
```

#### 3. Searching (`find`)

Find symbols by name pattern.

```bash
# Find all symbols matching "User*"
kampus find "User*"

# Find only functions in Rust files
kampus find "process_*" --kind function --language rs
```

#### 4. Call Graphs (`calls`)

Analyze function calls.

```bash
# Show callers and callees of a function
kampus calls main

# Show only callers (who calls this function?)
kampus calls sensitive_operation --direction callers
```

#### 5. Custom Queries (`query`)

Execute raw Cypher queries against the graph.

```bash
# specific query
kampus query "MATCH (n:Function) RETURN n.name, n.file LIMIT 5"
```

#### 6. Status (`status`)

Show index statistics.

```bash
# Show summary stats
kampus status

# List all indexed files
kampus status --files
```

## Supported Languages

-   C++ (`cpp`)
-   Go (`go`)
-   JavaScript (`js`)
-   TypeScript (`ts`)
-   Python (`py`)
-   Rust (`rs`)
