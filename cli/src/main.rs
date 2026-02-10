use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};

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
        #[arg(short = 'f', long = "file")]
        file: Option<PathBuf>,
        urls: Vec<String>,
    },
}

#[derive(Deserialize, Serialize)]
struct Config {
    base_url: String,
    ingest_token: Option<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    total_hits: u64,
    results: Vec<SearchResultItem>,
}

#[derive(Deserialize)]
struct SearchResultItem {
    url: String,
    title: Option<String>,
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
            handle_query_response(response).await?;
        }
        Commands::Ingest { file, urls } => {
            let mut ingest_urls = Vec::new();
            ingest_urls.extend(urls);
            if let Some(path) = file {
                let contents = fs::read_to_string(&path).with_context(|| {
                    format!("failed to read ingest file {}", path.display())
                })?;
                ingest_urls.extend(
                    contents
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(str::to_string),
                );
            }

            if ingest_urls.is_empty() {
                anyhow::bail!("provide at least one url or a non-empty file to ingest");
            }
            let mut headers = HeaderMap::new();
            if let Some(token) = config.ingest_token.as_deref() {
                headers.insert(AUTHORIZATION, auth_header(token)?);
            }

            let response = client
                .post(format!("{}/v1/ingest/urls", base_url))
                .headers(headers)
                .json(&serde_json::json!({ "urls": ingest_urls }))
                .send()
                .await
                .context("failed to send ingest request")?;
            handle_response(response).await?;
        }
    }

    Ok(())
}

fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        let config = default_config();
        write_config(path, &config)?;
        return Ok(config);
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let config: Config = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse config file {}", path.display()))?;
    Ok(config)
}

fn default_config() -> Config {
    Config {
        base_url: "http://localhost:3000".to_string(),
        ingest_token: None,
    }
}

fn write_config(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
    }
    let raw =
        serde_json::to_string_pretty(config).context("failed to serialize config file")?;
    fs::write(path, raw)
        .with_context(|| format!("failed to write config file {}", path.display()))?;
    Ok(())
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

async fn handle_query_response(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await.context("failed to read response")?;
    if !status.is_success() {
        anyhow::bail!("request failed with status {}: {}", status, body);
    }
    let response: SearchResponse =
        serde_json::from_str(&body).context("failed to parse search response")?;

    if response.results.is_empty() {
        println!("No results.");
        return Ok(());
    }

    for (index, item) in response.results.iter().enumerate() {
        let title = item
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(item.url.as_str());
        let label = if item.url.trim().is_empty() {
            title.to_string()
        } else {
            hyperlink(&item.url, title)
        };
        println!("{:>2}. {}", index + 1, label);
    }

    Ok(())
}

fn hyperlink(url: &str, text: &str) -> String {
    format!("\u{1b}]8;;{}\u{1b}\\{}\u{1b}]8;;\u{1b}\\", url, text)
}
