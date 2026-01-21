//! Index command implementation

use kampus_core::index::full::{FullIndexConfig, FullIndexer};
use kampus_core::Language;
use std::path::PathBuf;

pub async fn run(
    path: &str,
    jobs: Option<usize>,
    languages: Option<&str>,
    clear_existing: bool,
    db_uri: Option<&str>,
    graph_name: &str,
) -> anyhow::Result<()> {
    let languages = parse_languages(languages)?;

    let config = FullIndexConfig {
        root: PathBuf::from(path),
        languages,
        threads: jobs.unwrap_or_else(num_cpus::get),
        clear_existing,
        db_uri: db_uri.map(String::from),
        graph_name: graph_name.to_string(),
    };

    let indexer = FullIndexer::new(config);
    let stats = indexer.run().await?;

    println!("\n{}", stats);
    Ok(())
}

fn parse_languages(input: Option<&str>) -> anyhow::Result<Option<Vec<Language>>> {
    match input {
        None => Ok(None),
        Some(s) => {
            let languages: Result<Vec<Language>, _> = s
                .split(',')
                .map(|lang| lang.trim().parse::<Language>())
                .collect();

            match languages {
                Ok(langs) => Ok(Some(langs)),
                Err(e) => Err(anyhow::anyhow!("Invalid language: {}", e)),
            }
        }
    }
}
