use crate::errors::AppError;
use crate::types::{AppState, BookmarksResponse};
use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;

pub(super) async fn list_bookmarks(
    State(state): State<AppState>,
) -> Result<Json<BookmarksResponse>, AppError> {
    let response = state.services.bookmarks.list().await?;
    Ok(Json(response))
}

pub(super) async fn delete_bookmark(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    state.services.auth.authorize(&headers)?;
    state.services.bookmarks.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
