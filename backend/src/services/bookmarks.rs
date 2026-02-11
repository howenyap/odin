use std::sync::Arc;

use tantivy::Term;
use tracing::info;

use crate::errors::AppError;
use crate::types::{Dependencies, BookmarkListItem, BookmarksResponse};

#[derive(Clone)]
pub struct BookmarkService {
    deps: Arc<Dependencies>,
}

impl BookmarkService {
    pub fn new(deps: Arc<Dependencies>) -> Self {
        Self { deps }
    }

    pub async fn list(&self) -> Result<BookmarksResponse, AppError> {
        let results: Vec<BookmarkListItem> = sqlx::query_as(
            r#"
            SELECT id, url, title, status, updated_at
            FROM bookmarks  
            ORDER BY updated_at DESC, id DESC
            "#,
        )
        .fetch_all(&self.deps.db)
        .await?;

        info!("bookmarks listed: {}", results.len());
        Ok(BookmarksResponse { results })
    }

    pub async fn delete(&self, id: i64) -> Result<(), AppError> {
        info!("bookmark delete requested: id={}", id);
        if id <= 0 {
            return Err(AppError::bad_request("invalid bookmark id"));
        }

        let url: Option<String> = sqlx::query_scalar("SELECT url FROM bookmarks WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.deps.db)
            .await?;
        let Some(url) = url else {
            info!("bookmark delete not found: id={}", id);
            return Err(AppError::not_found("bookmark not found"));
        };

        {
            let mut writer = self.deps.writer.lock().await;
            writer.delete_term(Term::from_field_text(self.deps.fields.url, &url));
            writer.commit()?;
            self.deps.reader.reload()?;
        }

        let result = sqlx::query("DELETE FROM bookmarks WHERE id = ?1")
            .bind(id)
            .execute(&self.deps.db)
            .await?;
        if result.rows_affected() == 0 {
            info!("bookmark delete missing row after select: id={}", id);
            return Err(AppError::not_found("bookmark not found"));
        }

        info!("bookmark deleted: id={} url={}", id, url);
        Ok(())
    }
}
