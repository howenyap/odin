use axum::Json;
use axum::extract::{Query, State};
use crate::errors::AppError;
use crate::types::{AppState, SearchParams, SearchResponse};

pub(super) async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, AppError> {
    let response = state.services.search.search(params).await?;
    Ok(Json(response))
}
