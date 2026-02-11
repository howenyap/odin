use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;

use crate::errors::AppError;
use crate::types::{AppState, IngestUrlsRequest, IngestUrlsResponse};

pub(super) async fn ingest_urls(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<IngestUrlsRequest>,
) -> Result<Json<IngestUrlsResponse>, AppError> {
    state.services.auth.authorize(&headers)?;
    let response = state.services.ingest.ingest_urls(payload).await?;
    Ok(Json(response))
}
