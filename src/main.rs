use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqlitePoolOptions;
use tantivy::Index;
use tantivy::schema::{STORED, STRING, Schema, TEXT};
use tokio::sync::{Mutex, Semaphore};
use tracing::info;

mod error;
mod routes;
mod r#type;

use crate::routes::build_router;
use crate::r#type::{AppState, IndexFields};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let data_dir = PathBuf::from("data");
    let index_dir = data_dir.join("index");
    let db_path = data_dir.join("app.db");

    tokio::fs::create_dir_all(&data_dir)
        .await
        .context("create data dir")?;
    tokio::fs::create_dir_all(&index_dir)
        .await
        .context("create index dir")?;

    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_path)
                .create_if_missing(true),
        )
        .await
        .context("connect sqlite")?;

    init_db(&db).await?;

    let (schema, fields) = build_schema();
    let index =
        Index::open_or_create(tantivy::directory::MmapDirectory::open(&index_dir)?, schema)?;
    let reader = index.reader()?;
    let writer = index.writer(50_000_000)?;

    let http_client = reqwest::Client::builder()
        .user_agent("odin/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("build http client")?;

    let state = AppState {
        db,
        index,
        reader,
        writer: Arc::new(Mutex::new(writer)),
        fields,
        fetch_semaphore: Arc::new(Semaphore::new(10)),
        http_client,
    };

    let app = build_router(state);

    let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

fn build_schema() -> (Schema, IndexFields) {
    let mut schema_builder = Schema::builder();
    let url = schema_builder.add_text_field("url", STRING | STORED);
    let title = schema_builder.add_text_field("title", TEXT | STORED);
    let body = schema_builder.add_text_field("body", TEXT);
    let excerpt = schema_builder.add_text_field("excerpt", STORED);
    let fetched_at = schema_builder.add_i64_field("fetched_at", STORED);
    let schema = schema_builder.build();
    (
        schema,
        IndexFields {
            url,
            title,
            body,
            excerpt,
            fetched_at,
        },
    )
}

async fn init_db(db: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bookmarks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT NOT NULL UNIQUE,
            title TEXT,
            excerpt TEXT,
            status TEXT NOT NULL,
            http_status INTEGER,
            content_type TEXT,
            error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            fetched_at TEXT,
            indexed_at TEXT
        );
        "#,
    )
    .execute(db)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_bookmarks_status ON bookmarks(status);")
        .execute(db)
        .await?;

    Ok(())
}
