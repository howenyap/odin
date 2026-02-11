use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(name = "odin", about = "CLI for querying and ingesting URLs")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Config,
    Query {
        query: String,
    },
    List,
    Delete {
        id: i64,
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
    #[serde(alias = "ingest_token")]
    admin_token: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:3000".to_string(),
            admin_token: None,
        }
    }
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

#[derive(Deserialize)]
struct BookmarksResponse {
    results: Vec<BookmarkListItem>,
}

#[derive(Deserialize)]
struct BookmarkListItem {
    id: i64,
    url: String,
    title: Option<String>,
    status: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = resolve_config_path(cli.config);
    let config = load_config(&config_path)?;
    let base_url = config.base_url.trim_end_matches('/');

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build http client")?;
    match cli.command {
        Commands::Config => {
            println!("{}", config_path.display());
        }
        Commands::Query { query } => {
            let response = client
                .get(format!("{}/v1/search", base_url))
                .query(&[("query", query)])
                .send()
                .await
                .context("failed to send query request")?;
            handle_query_response(response).await?;
        }
        Commands::List => {
            let response = client
                .get(format!("{}/v1/bookmarks", base_url))
                .send()
                .await
                .context("failed to send bookmarks request")?;
            handle_bookmarks_response(response).await?;
        }
        Commands::Delete { id } => {
            let token = config
                .admin_token
                .as_deref()
                .context("admin_token missing in config; required for delete")?;
            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, auth_header(token)?);

            let response = client
                .delete(format!("{}/v1/bookmarks/{}", base_url, id))
                .headers(headers)
                .send()
                .await
                .context("failed to send delete request")?;
            handle_delete_response(response, id).await?;
        }
        Commands::Ingest { file, urls } => {
            let mut ingest_urls = Vec::new();
            ingest_urls.extend(urls);
            if let Some(path) = file {
                let contents = fs::read_to_string(&path)
                    .with_context(|| format!("failed to read ingest file {}", path.display()))?;
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
            if let Some(token) = config.admin_token.as_deref() {
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

fn resolve_config_path(config_arg: Option<PathBuf>) -> PathBuf {
    config_arg.unwrap_or_else(default_config_path)
}

fn default_config_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("odin").join("config.json");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("odin")
            .join("config.json");
    }
    PathBuf::from("config.json")
}

fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        let config = Config::default();
        write_config(path, &config)?;
        return Ok(config);
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let config: Config = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse config file {}", path.display()))?;
    Ok(config)
}

fn write_config(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(config).context("failed to serialize config file")?;
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
    HeaderValue::from_str(&value).context("invalid admin token")
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

    println!(
        "Found {} result{}.",
        response.total_hits,
        if response.total_hits == 1 { "" } else { "s" }
    );

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

async fn handle_bookmarks_response(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await.context("failed to read response")?;
    if !status.is_success() {
        anyhow::bail!("request failed with status {}: {}", status, body);
    }
    let response: BookmarksResponse =
        serde_json::from_str(&body).context("failed to parse bookmarks response")?;

    if response.results.is_empty() {
        println!("No bookmarks.");
        return Ok(());
    }

    let id_width = response
        .results
        .iter()
        .map(|item| item.id.to_string().len())
        .max()
        .unwrap_or(2)
        .max("ID".len());
    let status_width = response
        .results
        .iter()
        .map(|item| item.status.len())
        .max()
        .unwrap_or(6)
        .max("Status".len());
    let mut title_width = response
        .results
        .iter()
        .map(|item| {
            item.title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(item.url.as_str())
                .len()
        })
        .max()
        .unwrap_or(5)
        .max("Title".len());
    let title_width_cap = 80usize;
    if title_width > title_width_cap {
        title_width = title_width_cap;
    }

    println!(
        "{:>id_width$}  {:<status_width$}  {:<title_width$}",
        "ID",
        "Status",
        "Title"
    );
    println!(
        "{:-<id_width$}  {:-<status_width$}  {:-<title_width$}",
        "",
        "",
        ""
    );

    for (index, item) in response.results.iter().enumerate() {
        let title = item
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(item.url.as_str());
        let title = truncate_with_ellipsis(title, title_width);
        println!(
            "{:>id_width$}  {:<status_width$}  {:<title_width$}",
            item.id,
            item.status,
            title
        );
    }

    Ok(())
}

async fn handle_delete_response(response: reqwest::Response, id: i64) -> Result<()> {
    let status = response.status();
    let body = response.text().await.context("failed to read response")?;
    if status == reqwest::StatusCode::NO_CONTENT {
        println!("Deleted bookmark {}.", id);
        return Ok(());
    }
    if !status.is_success() {
        anyhow::bail!("request failed with status {}: {}", status, body);
    }
    if body.trim().is_empty() {
        println!("Deleted bookmark {}.", id);
        return Ok(());
    }
    println!("{}", body);
    Ok(())
}

fn hyperlink(url: &str, text: &str) -> String {
    if std::io::stdout().is_terminal() {
        format!("\u{1b}]8;;{}\u{1b}\\{}\u{1b}]8;;\u{1b}\\", url, text)
    } else {
        text.to_string()
    }
}

fn truncate_with_ellipsis(value: &str, max_width: usize) -> String {
    if value.len() <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return value.chars().take(max_width).collect();
    }
    format!("{}...", value.chars().take(max_width - 3).collect::<String>())
}
