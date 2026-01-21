//! Calls command implementation - show call graph

use kampus_core::graph::{FalkorValue, GraphSchema};

pub async fn run(
    function: &str,
    direction: &str,
    depth: u32,
    db_uri: Option<&str>,
    graph_name: &str,
) -> anyhow::Result<()> {
    let schema = GraphSchema::connect(db_uri, graph_name).await?;

    match direction {
        "callers" => show_callers(&schema, function, depth).await?,
        "callees" => show_callees(&schema, function, depth).await?,
        "both" | _ => {
            println!("=== Callers (functions that call {}) ===\n", function);
            show_callers(&schema, function, depth).await?;
            println!("\n=== Callees (functions called by {}) ===\n", function);
            show_callees(&schema, function, depth).await?;
        }
    }

    Ok(())
}

async fn show_callers(
    schema: &GraphSchema,
    function: &str,
    depth: u32,
) -> anyhow::Result<()> {
    let cypher = format!(
        r#"
        MATCH (target:Function {{name: '{}'}})
        MATCH path = (caller:Function)-[:CALLS*1..{}]->(target)
        RETURN caller.name, caller.file_path, length(path)
        ORDER BY length(path), caller.name
        LIMIT 50
        "#,
        escape_string(function),
        depth
    );

    let results = schema.query(&cypher).await?;

    if results.is_empty() {
        println!("No callers found for '{}'", function);
        return Ok(());
    }

    println!("{:<30} {:<40} {:<8}", "CALLER", "FILE", "DISTANCE");
    println!("{}", "-".repeat(80));

    for row in &results {
        let name = extract_string(&row.get(0));
        let file = extract_string(&row.get(1));
        let distance = extract_i64(&row.get(2));

        let indent = "  ".repeat((distance - 1).max(0) as usize);
        println!(
            "{}{:<30} {:<40} {}",
            indent,
            truncate(&name, 30 - indent.len()),
            truncate(&file, 40),
            distance
        );
    }

    println!("\nFound {} caller(s).", results.len());
    Ok(())
}

async fn show_callees(
    schema: &GraphSchema,
    function: &str,
    depth: u32,
) -> anyhow::Result<()> {
    let cypher = format!(
        r#"
        MATCH (source:Function {{name: '{}'}})
        MATCH path = (source)-[:CALLS*1..{}]->(callee:Function)
        RETURN callee.name, callee.file_path, length(path)
        ORDER BY length(path), callee.name
        LIMIT 50
        "#,
        escape_string(function),
        depth
    );

    let results = schema.query(&cypher).await?;

    if results.is_empty() {
        println!("No callees found for '{}'", function);
        return Ok(());
    }

    println!("{:<30} {:<40} {:<8}", "CALLEE", "FILE", "DISTANCE");
    println!("{}", "-".repeat(80));

    for row in &results {
        let name = extract_string(&row.get(0));
        let file = extract_string(&row.get(1));
        let distance = extract_i64(&row.get(2));

        let indent = "  ".repeat((distance - 1).max(0) as usize);
        println!(
            "{}{:<30} {:<40} {}",
            indent,
            truncate(&name, 30 - indent.len()),
            truncate(&file, 40),
            distance
        );
    }

    println!("\nFound {} callee(s).", results.len());
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
    if max_len < 4 {
        return s.chars().take(max_len).collect();
    }
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
