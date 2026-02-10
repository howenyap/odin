use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tantivy::schema::Field;
use tantivy::{Index, IndexReader, IndexWriter};
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub index: Index,
    pub reader: IndexReader,
    pub writer: Arc<Mutex<IndexWriter>>,
    pub fields: IndexFields,
    pub fetch_semaphore: Arc<Semaphore>,
    pub http_client: reqwest::Client,
}

#[derive(Clone, Copy)]
pub struct IndexFields {
    pub url: Field,
    pub title: Field,
    pub body: Field,
    pub excerpt: Field,
    pub fetched_at: Field,
}

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: String,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub total_hits: u64,
    pub results: Vec<SearchResultItem>,
}

#[derive(Serialize)]
pub struct SearchResultItem {
    pub url: String,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub score: f32,
}

#[derive(Deserialize)]
pub struct IngestUrlsRequest {
    pub urls: Vec<String>,
}

#[derive(Serialize)]
pub struct IngestUrlsResponse {
    pub accepted: usize,
    pub deduped: usize,
}
