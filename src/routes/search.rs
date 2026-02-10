use axum::extract::{Query, State};
use axum::Json;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::QueryParser;
use tantivy::schema::{TantivyDocument, Value};
use tantivy::TantivyError;
use tracing::info;

use crate::error::AppError;
use crate::r#type::{AppState, SearchParams, SearchResponse, SearchResultItem};

pub(super) async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, AppError> {
    let query = params.q.trim();
    info!(
        "search request received: q='{}' page={:?} per_page={:?}",
        query,
        params.page,
        params.per_page
    );
    if query.is_empty() {
        return Ok(Json(SearchResponse {
            total_hits: 0,
            results: vec![],
        }));
    }

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(10).clamp(1, 50);
    let offset = ((page - 1) * per_page) as usize;

    let searcher = state.reader.searcher();
    let query_parser =
        QueryParser::for_index(&state.index, vec![state.fields.title, state.fields.body]);
    let tantivy_query = query_parser
        .parse_query(query)
        .map_err(|err| AppError::bad_request(err.to_string()))?;

    let total_hits = searcher.search(&tantivy_query, &Count)? as u64;
    let top_docs = searcher.search(
        &tantivy_query,
        &TopDocs::with_limit(per_page as usize).and_offset(offset),
    )?;

    let results = top_docs
        .into_iter()
        .map(|(score, doc_address)| {
            let retrieved: TantivyDocument = searcher.doc(doc_address)?;
            let url = retrieved
                .get_first(state.fields.url)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = retrieved
                .get_first(state.fields.title)
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let excerpt = retrieved
                .get_first(state.fields.excerpt)
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());

            Ok(SearchResultItem {
                url,
                title,
                excerpt,
                score,
            })
        })
        .collect::<Result<Vec<_>, TantivyError>>()?;

    info!(
        "search completed: q='{}' total_hits={} returned={}",
        query,
        total_hits,
        results.len()
    );
    Ok(Json(SearchResponse {
        total_hits,
        results,
    }))
}
