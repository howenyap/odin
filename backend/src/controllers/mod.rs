use axum::Router;
use axum::http::Method;
use axum::routing::{delete, get, post};
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::types::AppState;

mod bookmarks;
mod healthz;
mod ingest;
mod search;

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    Router::new()
        .route("/healthz", get(healthz::healthz))
        .route("/v1/search", get(search::search))
        .route("/v1/bookmarks", get(bookmarks::list_bookmarks))
        .route("/v1/bookmarks/{id}", delete(bookmarks::delete_bookmark))
        .route("/v1/ingest/urls", post(ingest::ingest_urls))
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
