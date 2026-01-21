//! Status command implementation

use kampus_core::graph::{FalkorValue, GraphSchema};

pub async fn run(show_files: bool, db_uri: Option<&str>, graph_name: &str) -> anyhow::Result<()> {
    let schema = GraphSchema::connect(db_uri, graph_name).await?;

    // Get statistics
    let stats = schema.stats().await?;
    println!("Index Status for graph '{}'", graph_name);
    println!("================================\n");
    println!("{}", stats);

    // Get last indexed commit
    if let Some(commit) = schema.get_metadata("last_indexed_commit").await? {
        println!("Last indexed commit: {}", commit);
    } else {
        println!("Last indexed commit: (none)");
    }

    // Show files if requested
    if show_files {
        println!("\n--- Indexed Files ---\n");

        let results = schema.query(
            r#"
            MATCH (f:File)
            RETURN f.path, f.language, f.line_count
            ORDER BY f.path
            "#,
        ).await?;

        if results.is_empty() {
            println!("No files indexed.");
        } else {
            println!("{:<60} {:<12} {:<8}", "PATH", "LANGUAGE", "LINES");
            println!("{}", "-".repeat(82));

            for row in &results {
                let path = extract_string(&row.get(0));
                let language = extract_string(&row.get(1));
                let lines = extract_i64(&row.get(2));

                println!(
                    "{:<60} {:<12} {:<8}",
                    truncate(&path, 60),
                    language,
                    lines
                );
            }

            println!("\nTotal: {} files", results.len());
        }
    }

    // Show language breakdown
    println!("\n--- Files by Language ---\n");

    let results = schema.query(
        r#"
        MATCH (f:File)
        RETURN f.language, count(f)
        ORDER BY count(f) DESC
        "#,
    ).await?;

    for row in &results {
        let language = extract_string(&row.get(0));
        let count = extract_i64(&row.get(1));
        println!("  {:<12} {}", language, count);
    }

    Ok(())
}

fn extract_string(val: &Option<&FalkorValue>) -> String {
    val.and_then(|v| match v {
        FalkorValue::String(s) => Some(s.clone()),
        _ => None,
    })
    .unwrap_or_default()
}

fn extract_i64(val: &Option<&FalkorValue>) -> i64 {
    val.and_then(|v| match v {
        FalkorValue::I64(n) => Some(*n),
        _ => None,
    })
    .unwrap_or(0)
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
