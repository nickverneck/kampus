//! Query command implementation

use kampus_core::graph::{FalkorValue, GraphSchema};

pub async fn run(
    cypher: &str,
    format: &str,
    db_uri: Option<&str>,
    graph_name: &str,
) -> anyhow::Result<()> {
    let schema = GraphSchema::connect(db_uri, graph_name).await?;
    let results = schema.query(cypher).await?;

    match format {
        "json" => {
            // Convert FalkorValue to serde_json::Value
            let json_results: Vec<serde_json::Value> = results
                .iter()
                .map(|row| {
                    let json_row: Vec<serde_json::Value> = row
                        .iter()
                        .map(falkor_to_json)
                        .collect();
                    serde_json::Value::Array(json_row)
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_results)?);
        }
        "table" | _ => {
            if results.is_empty() {
                println!("No results found.");
            } else {
                // Simple table output
                for (i, row) in results.iter().enumerate() {
                    println!("--- Row {} ---", i + 1);
                    for (j, value) in row.iter().enumerate() {
                        println!("  [{}]: {:?}", j, value);
                    }
                }
                println!("\n{} row(s) returned.", results.len());
            }
        }
    }

    Ok(())
}

fn falkor_to_json(value: &FalkorValue) -> serde_json::Value {
    match value {
        FalkorValue::String(s) => serde_json::Value::String(s.clone()),
        FalkorValue::I64(n) => serde_json::Value::Number((*n).into()),
        FalkorValue::F64(n) => serde_json::json!(n),
        FalkorValue::Bool(b) => serde_json::Value::Bool(*b),
        FalkorValue::None => serde_json::Value::Null,
        FalkorValue::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(falkor_to_json).collect())
        }
        FalkorValue::Map(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), falkor_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        FalkorValue::Node(node) => {
            serde_json::json!({
                "type": "node",
                "id": node.entity_id,
                "labels": node.labels,
                "properties": node.properties.iter()
                    .map(|(k, v)| (k.clone(), falkor_to_json(v)))
                    .collect::<serde_json::Map<_, _>>()
            })
        }
        FalkorValue::Edge(edge) => {
            serde_json::json!({
                "type": "edge",
                "id": edge.entity_id,
                "relationship_type": edge.relationship_type,
                "src_node": edge.src_node_id,
                "dst_node": edge.dst_node_id,
                "properties": edge.properties.iter()
                    .map(|(k, v)| (k.clone(), falkor_to_json(v)))
                    .collect::<serde_json::Map<_, _>>()
            })
        }
        _ => serde_json::json!(format!("{:?}", value)),
    }
}
