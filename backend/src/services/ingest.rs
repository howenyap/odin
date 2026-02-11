use std::sync::Arc;

use reqwest::header::CONTENT_TYPE;
use scraper::{Html, Selector};
use tantivy::{Term, doc};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::{error, info};
use url::Url;

use crate::errors::AppError;
use crate::types::{Dependencies, IngestUrlsRequest, IngestUrlsResponse};

#[derive(Clone)]
pub struct IngestService {
    deps: Arc<Dependencies>,
}

impl IngestService {
    const MAX_URLS: usize = 100;

    pub fn new(deps: Arc<Dependencies>) -> Self {
        Self { deps }
    }

    pub async fn ingest_urls(
        &self,
        payload: IngestUrlsRequest,
    ) -> Result<IngestUrlsResponse, AppError> {
        info!("ingest request received: {} urls", payload.urls.len());

        if payload.urls.is_empty() {
            return Ok(IngestUrlsResponse {
                accepted: 0,
                deduped: 0,
            });
        }

        if payload.urls.len() > Self::MAX_URLS {
            return Err(AppError::bad_request("too many urls"));
        }

        let mut accepted = 0usize;
        let mut deduped = 0usize;

        for raw_url in payload.urls {
            let Some(normalized) = Self::normalize_url(&raw_url) else {
                deduped += 1;
                continue;
            };

            let now = Self::now_rfc3339();
            let result = sqlx::query(
                r#"
                INSERT OR IGNORE INTO bookmarks (url, title, excerpt, status, http_status, content_type, error, created_at, updated_at, fetched_at, indexed_at)
                VALUES (?1, NULL, NULL, 'queued', NULL, NULL, NULL, ?2, ?2, NULL, NULL)
                "#,
            )
            .bind(&normalized)
            .bind(&now)
            .execute(&self.deps.db)
            .await?;

            if result.rows_affected() == 0 {
                deduped += 1;
                continue;
            }

            accepted += 1;
            let service = self.clone();

            tokio::spawn(async move {
                if let Err(err) = service.process_url(normalized).await {
                    error!("ingest error: {:?}", err);
                }
            });
        }

        Ok(IngestUrlsResponse { accepted, deduped })
    }
    /// Fetch, parse, index, and persist a single URL.
    async fn process_url(&self, url: String) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        info!("ingest start: {}", url);
        let _permit = self.deps.fetch_semaphore.acquire().await?;

        let response = match self.deps.http_client.get(&url).send().await {
            Ok(response) => response,
            Err(err) => {
                self.mark_failed(&url, 0, "", &Self::truncate_error(&err.to_string()))
                    .await?;
                info!(
                    "ingest end: {} status=failed reason=request_error elapsed_ms={}",
                    url,
                    start.elapsed().as_millis()
                );

                return Ok(());
            }
        };

        let status = response.status();
        let http_status = status.as_u16();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string())
            .unwrap_or_default();

        let body = match response.bytes().await {
            Ok(body) => body,
            Err(err) => {
                self.mark_failed(
                    &url,
                    http_status,
                    &content_type,
                    &Self::truncate_error(&err.to_string()),
                )
                .await?;
                info!(
                    "ingest end: {} status=failed reason=read_body_error error={} elapsed_ms={}",
                    url,
                    err,
                    start.elapsed().as_millis()
                );
                return Ok(());
            }
        };

        if !status.is_success() {
            let mut message = format!("http error: {}", status);
            if let Some(preview) = Self::body_preview(&body) {
                message.push_str(&format!(" body_preview={}", preview));
            }
            self.mark_failed(
                &url,
                http_status,
                &content_type,
                &Self::truncate_error(&message),
            )
            .await?;
            info!(
                "ingest end: {} status=failed reason=http_error http_status={} elapsed_ms={}",
                url,
                http_status,
                start.elapsed().as_millis()
            );
            return Ok(());
        }

        if !Self::is_html_content(&content_type, &body) {
            self.mark_failed(&url, http_status, &content_type, "unsupported content type")
                .await?;
            info!(
                "ingest end: {} status=failed reason=unsupported_content_type content_type={} elapsed_ms={}",
                url,
                content_type,
                start.elapsed().as_millis()
            );
            return Ok(());
        }

        let html = String::from_utf8_lossy(&body).to_string();
        let (title, body) = Self::extract_text(&html);
        let cleaned = Self::clean_text(&body);
        let excerpt = Self::make_excerpt(&cleaned, 280);

        if let Err(err) = self.index_document(&url, &title, &cleaned, &excerpt).await {
            self.mark_failed(&url, http_status, &content_type, &err.to_string())
                .await?;
            info!(
                "ingest end: {} status=failed reason=index_error error={} elapsed_ms={}",
                url,
                err,
                start.elapsed().as_millis()
            );
            return Ok(());
        }

        let now = Self::now_rfc3339();
        if let Err(err) = sqlx::query(
            r#"
            UPDATE bookmarks
            SET title = ?1, excerpt = ?2, status = 'indexed', http_status = ?3, content_type = ?4, error = NULL,
                updated_at = ?5, fetched_at = ?5, indexed_at = ?5
            WHERE url = ?6
            "#,
        )
        .bind(title.as_deref())
        .bind(excerpt.as_deref())
        .bind(http_status)
        .bind(content_type)
        .bind(&now)
        .bind(&url)
        .execute(&self.deps.db)
        .await
        {
            info!(
                "ingest end: {} status=failed reason=db_update_error error={} elapsed_ms={}",
                url,
                err,
                start.elapsed().as_millis()
            );
            return Ok(());
        }

        info!(
            "ingest end: {} status=indexed http_status={} elapsed_ms={}",
            url,
            http_status,
            start.elapsed().as_millis()
        );
        Ok(())
    }

    /// Write the fetched document into the Tantivy index.
    async fn index_document(
        &self,
        url: &str,
        title: &Option<String>,
        body: &str,
        excerpt: &Option<String>,
    ) -> anyhow::Result<()> {
        let mut writer = self.deps.writer.lock().await;

        writer.delete_term(Term::from_field_text(self.deps.fields.url, url));

        let fetched_at = OffsetDateTime::now_utc().unix_timestamp();
        let doc = doc!(
            self.deps.fields.url => url,
            self.deps.fields.title => title.clone().unwrap_or_default(),
            self.deps.fields.body => body,
            self.deps.fields.excerpt => excerpt.clone().unwrap_or_default(),
            self.deps.fields.fetched_at => fetched_at,
        );

        writer.add_document(doc)?;
        writer.commit()?;
        self.deps.reader.reload()?;
        Ok(())
    }

    /// Mark a bookmark as failed with the provided HTTP and error details.
    async fn mark_failed(
        &self,
        url: &str,
        http_status: u16,
        content_type: &str,
        error: &str,
    ) -> anyhow::Result<()> {
        let now = Self::now_rfc3339();
        sqlx::query(
            r#"
            UPDATE bookmarks
            SET status = 'failed', http_status = ?1, content_type = ?2, error = ?3, updated_at = ?4, fetched_at = ?4
            WHERE url = ?5
            "#,
        )
        .bind(http_status)
        .bind(content_type)
        .bind(error)
        .bind(&now)
        .bind(url)
        .execute(&self.deps.db)
        .await?;
        Ok(())
    }

    /// Extract a best-effort title and raw body text from HTML.
    fn extract_text(html: &str) -> (Option<String>, String) {
        let document = Html::parse_document(html);
        let title = Self::extract_title(&document);
        let body = html2text::from_read(html.as_bytes(), 80);

        (title, body)
    }

    /// Prefer OpenGraph/H1/title metadata for the page title.
    fn extract_title(document: &Html) -> Option<String> {
        let og_title_selector = Selector::parse(r#"meta[property="og:title"]"#).unwrap();
        let h1_selector = Selector::parse("h1").unwrap();
        let title_selector = Selector::parse("title").unwrap();

        let candidates = [
            Self::select_meta_content(document, &og_title_selector),
            Self::select_text(document, &h1_selector),
            Self::select_text(document, &title_selector).and_then(|t| Self::trim_site_suffix(&t)),
        ];

        candidates.into_iter().flatten().next()
    }

    /// Read a meta tag's `content` attribute and normalize whitespace.
    fn select_meta_content(document: &Html, selector: &Selector) -> Option<String> {
        document
            .select(selector)
            .next()
            .and_then(|node| node.value().attr("content"))
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    /// Read the first matching node's text content and normalize whitespace.
    fn select_text(document: &Html, selector: &Selector) -> Option<String> {
        document
            .select(selector)
            .next()
            .map(|node| node.text().collect::<Vec<_>>().join(" "))
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    /// Remove common site-name suffixes while preserving short titles.
    fn trim_site_suffix(title: &str) -> Option<String> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return None;
        }

        for delimiter in [" — ", " | ", " - ", " · "] {
            if let Some((left, _)) = trimmed.split_once(delimiter) {
                let left = left.trim();
                if left.len() >= 6 {
                    return Some(left.to_string());
                }
            }
        }

        Some(trimmed.to_string())
    }

    /// Collapse whitespace runs and trim the output.
    fn clean_text(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut prev_space = false;
        for ch in input.chars() {
            if ch.is_whitespace() {
                if !prev_space {
                    out.push(' ');
                    prev_space = true;
                }
            } else {
                prev_space = false;
                out.push(ch);
            }
        }
        out.trim().to_string()
    }

    /// Build a short excerpt for display or error contexts.
    fn make_excerpt(text: &str, max_len: usize) -> Option<String> {
        if text.is_empty() {
            None
        } else {
            Some(
                text.chars()
                    .take(max_len)
                    .chain(std::iter::once('…'))
                    .collect::<String>(),
            )
        }
    }

    /// Trim and normalize a URL string, stripping fragments.
    fn normalize_url(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut url = Url::parse(trimmed).ok()?;
        url.set_fragment(None);
        Some(url.to_string())
    }

    /// Return the current UTC timestamp in RFC 3339 format.
    fn now_rfc3339() -> String {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .expect("failed to format timestamp")
    }

    /// Determine whether a response is likely HTML.
    fn is_html_content(content_type: &str, body: &[u8]) -> bool {
        let ct = content_type.trim().to_ascii_lowercase();
        if ct.starts_with("text/html") || ct.starts_with("application/xhtml+xml") {
            return true;
        }
        if ct.is_empty()
            || ct.starts_with("text/plain")
            || ct.starts_with("application/octet-stream")
        {
            return Self::looks_like_html(body);
        }
        false
    }

    /// Heuristically detect HTML from a short body prefix.
    fn looks_like_html(body: &[u8]) -> bool {
        let prefix = &body[..body.len().min(512)];
        let s = String::from_utf8_lossy(prefix);
        let trimmed = s
            .trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
            .to_ascii_lowercase();

        [
            "<!doctype",
            "<html",
            "<head",
            "<body",
            "<meta",
            "<title",
            "<",
        ]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    }

    /// Build a compact, human-readable preview for error messages.
    fn body_preview(body: &[u8]) -> Option<String> {
        let prefix = &body[..body.len().min(600)];
        let s = String::from_utf8_lossy(prefix);
        let mut out = Self::clean_text(&s);
        if out.is_empty() {
            return None;
        }
        if out.chars().count() > 240 {
            out = out.chars().take(240).collect::<String>();
            out.push('…');
        }
        Some(out)
    }

    /// Truncate long error messages for storage.
    fn truncate_error(message: &str) -> String {
        const MAX_CHARS: usize = 900;

        if message.chars().count() <= MAX_CHARS {
            message.to_string()
        } else {
            message
                .chars()
                .take(MAX_CHARS)
                .chain(std::iter::once('…'))
                .collect::<String>()
        }
    }
}
