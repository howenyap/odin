use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "odin", about = "CLI for querying and ingesting URLs")]
struct Cli {
    #[arg(long, default_value = "config.json")]
    config: PathBuf,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Query {
        query: String,
    },
    Ingest {
        urls: Vec<String>,
    },
}

#[derive(Deserialize)]
struct Config {
    base_url: String,
    ingest_token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli.config)?;
    let base_url = config.base_url.trim_end_matches('/');

    let client = reqwest::Client::new();
    match cli.command {
        Commands::Query { query } => {
            let response = client
                .get(format!("{}/v1/search", base_url))
                .query(&[("q", query)])
                .send()
                .await
                .context("failed to send query request")?;
            handle_response(response).await?;
        }
        Commands::Ingest { urls } => {
            if urls.is_empty() {
                anyhow::bail!("provide at least one url to ingest");
            }
            let mut headers = HeaderMap::new();
            if let Some(token) = config.ingest_token.as_deref() {
                headers.insert(AUTHORIZATION, auth_header(token)?);
            }

            let response = client
                .post(format!("{}/v1/ingest/urls", base_url))
                .headers(headers)
                .json(&serde_json::json!({ "urls": urls }))
                .send()
                .await
                .context("failed to send ingest request")?;
            handle_response(response).await?;
        }
    }

    Ok(())
}

fn load_config(path: &Path) -> Result<Config> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let config: Config = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse config file {}", path.display()))?;
    Ok(config)
}

fn auth_header(token: &str) -> Result<HeaderValue> {
    let value = if token.starts_with("Bearer ") {
        token.to_string()
    } else {
        format!("Bearer {}", token)
    };
    HeaderValue::from_str(&value).context("invalid ingest token")
}

async fn handle_response(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await.context("failed to read response")?;
    if !status.is_success() {
        anyhow::bail!("request failed with status {}: {}", status, body);
    }
    println!("{}", body);
    Ok(())
}
