//! Find command implementation

use kampus_core::graph::{FalkorValue, GraphSchema};

pub async fn run(
    pattern: &str,
    kind: Option<&str>,
    language: Option<&str>,
    limit: usize,
    db_uri: Option<&str>,
    graph_name: &str,
) -> anyhow::Result<()> {
    let schema = GraphSchema::connect(db_uri, graph_name).await?;

    // Build the Cypher query
    let label = match kind {
        Some("function") => "Function",
        Some("class") => "Class",
        Some("struct") => "Struct",
        Some("interface") => "Interface",
        Some("method") => "Method",
        Some("trait") => "Trait",
        Some("enum") => "Enum",
        Some(k) => return Err(anyhow::anyhow!("Unknown symbol kind: {}", k)),
        None => "", // Match any
    };

    // Convert wildcard pattern to FalkorDB-compatible WHERE clause
    // FalkorDB doesn't support regex, so we use CONTAINS/STARTS WITH/ENDS WITH
    let name_condition = pattern_to_condition(pattern);

    let cypher = if label.is_empty() {
        format!(
            r#"
            MATCH (s)
            WHERE (s:Function OR s:Class OR s:Struct OR s:Interface OR s:Method OR s:Trait OR s:Enum)
              AND {}
              {}
            RETURN s.name, labels(s)[0], s.file_path, s.start_line
            ORDER BY s.name
            LIMIT {}
            "#,
            name_condition,
            language.map(|l| format!("AND s.language = '{}'", l.to_uppercase())).unwrap_or_default(),
            limit
        )
    } else {
        format!(
            r#"
            MATCH (s:{})
            WHERE {}
              {}
            RETURN s.name, '{}', s.file_path, s.start_line
            ORDER BY s.name
            LIMIT {}
            "#,
            label,
            name_condition,
            language.map(|l| format!("AND s.language = '{}'", l.to_uppercase())).unwrap_or_default(),
            label,
            limit
        )
    };

    // Execute query
    let results = schema.query(&cypher).await?;

    // Display results
    if results.is_empty() {
        println!("No symbols found matching '{}'", pattern);
        return Ok(());
    }

    println!(
        "{:<30} {:<12} {:<40} {:<6}",
        "NAME", "KIND", "FILE", "LINE"
    );
    println!("{}", "-".repeat(90));

    for row in &results {
        let name = extract_string(&row.get(0));
        let kind = extract_string(&row.get(1));
        let file = extract_string(&row.get(2));
        let line = extract_i64(&row.get(3));

        println!(
            "{:<30} {:<12} {:<40} {:<6}",
            truncate(&name, 30),
            kind,
            truncate(&file, 40),
            line
        );
    }

    println!("\nFound {} symbol(s).", results.len());

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

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Convert a wildcard pattern to a FalkorDB WHERE condition
/// Supports * for any characters
/// Examples:
///   "auth" -> exact match (case-insensitive)
///   "*auth*" -> contains "auth"
///   "auth*" -> starts with "auth"
///   "*auth" -> ends with "auth"
fn pattern_to_condition(pattern: &str) -> String {
    let starts_with_wild = pattern.starts_with('*');
    let ends_with_wild = pattern.ends_with('*');

    // Remove wildcards to get the search term
    let search_term = pattern.trim_matches('*');
    let escaped = escape_string(search_term);
    let lower_term = escaped.to_lowercase();

    if search_term.is_empty() {
        // Just wildcards, match everything
        return "true".to_string();
    }

    match (starts_with_wild, ends_with_wild) {
        (true, true) => {
            // *term* -> contains
            format!("toLower(s.name) CONTAINS '{}'", lower_term)
        }
        (true, false) => {
            // *term -> ends with
            format!("toLower(s.name) ENDS WITH '{}'", lower_term)
        }
        (false, true) => {
            // term* -> starts with
            format!("toLower(s.name) STARTS WITH '{}'", lower_term)
        }
        (false, false) => {
            // exact match (case-insensitive)
            format!("toLower(s.name) = '{}'", lower_term)
        }
    }
}
