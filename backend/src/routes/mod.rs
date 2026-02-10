use axum::routing::{get, post};
use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::r#type::AppState;

mod healthz;
mod ingest;
mod search;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz::healthz))
        .route("/v1/search", get(search::search))
        .route("/v1/ingest/urls", post(ingest::ingest_urls))
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
