use axum::extract::State;
use axum::Json;
use sqlx::FromRow;
use tracing::info;

use crate::error::AppError;
use crate::r#type::{AppState, BookmarksResponse, BookmarkListItem};

#[derive(FromRow)]
struct BookmarkRow {
    url: String,
    title: Option<String>,
    status: String,
    updated_at: String,
}

pub(super) async fn list_bookmarks(
    State(state): State<AppState>,
) -> Result<Json<BookmarksResponse>, AppError> {
    let rows: Vec<BookmarkRow> = sqlx::query_as(
        r#"
        SELECT url, title, status, updated_at
        FROM bookmarks
        ORDER BY updated_at DESC, id DESC
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let results = rows
        .into_iter()
        .map(|row| BookmarkListItem {
            url: row.url,
            title: row.title,
            status: row.status,
            updated_at: row.updated_at,
        })
        .collect::<Vec<_>>();

    info!("bookmarks listed: {}", results.len());
    Ok(Json(BookmarksResponse { results }))
}
