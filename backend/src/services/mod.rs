mod auth;
mod bookmarks;
mod ingest;
mod search;

pub use auth::AuthService;
pub use bookmarks::BookmarkService;
pub use ingest::IngestService;
pub use search::SearchService;

use std::sync::Arc;

use crate::types::Dependencies;

#[derive(Clone)]
pub struct Services {
    pub auth: AuthService,
    pub bookmarks: BookmarkService,
    pub search: SearchService,
    pub ingest: IngestService,
}

impl Services {
    pub fn new(deps: Arc<Dependencies>) -> Self {
        Self {
            auth: AuthService::new(deps.clone()),
            bookmarks: BookmarkService::new(deps.clone()),
            search: SearchService::new(deps.clone()),
            ingest: IngestService::new(deps),
        }
    }
}
