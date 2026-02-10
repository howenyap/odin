use axum::extract::State;
use axum::Json;
use reqwest::header::CONTENT_TYPE;
use scraper::{Html, Selector};
use sqlx::SqlitePool;
use tantivy::{Term, doc};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::{error, info};
use url::Url;

use crate::error::AppError;
use crate::r#type::{AppState, IngestUrlsRequest, IngestUrlsResponse};

pub(super) async fn ingest_urls(
    State(state): State<AppState>,
    Json(payload): Json<IngestUrlsRequest>,
) -> Result<Json<IngestUrlsResponse>, AppError> {
    info!("ingest request received: {} urls", payload.urls.len());
    if payload.urls.is_empty() {
        return Ok(Json(IngestUrlsResponse {
            accepted: 0,
            deduped: 0,
        }));
    }
    if payload.urls.len() > 500 {
        return Err(AppError::bad_request("too many urls"));
    }

    let mut accepted = 0usize;
    let mut deduped = 0usize;

    for raw_url in payload.urls {
        let normalized = match normalize_url(&raw_url) {
            Some(u) => u,
            None => {
                deduped += 1;
                continue;
            }
        };

        let now = now_rfc3339();
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO bookmarks (url, title, excerpt, status, http_status, content_type, error, created_at, updated_at, fetched_at, indexed_at)
            VALUES (?1, NULL, NULL, 'queued', NULL, NULL, NULL, ?2, ?2, NULL, NULL)
            "#,
        )
        .bind(&normalized)
        .bind(&now)
        .execute(&state.db)
        .await?;

        if result.rows_affected() == 0 {
            deduped += 1;
            continue;
        }

        accepted += 1;
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(err) = process_url(state_clone, normalized).await {
                error!("ingest error: {:?}", err);
            }
        });
    }

    Ok(Json(IngestUrlsResponse { accepted, deduped }))
}

async fn process_url(state: AppState, url: String) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    info!("ingest start: {}", url);
    let _permit = state.fetch_semaphore.acquire().await?;

    let response = match state.http_client.get(&url).send().await {
        Ok(response) => response,
        Err(err) => {
            mark_failed(&state.db, &url, 0, "", &err.to_string()).await?;
            info!(
                "ingest end: {} status=failed reason=request_error elapsed_ms={}",
                url,
                start.elapsed().as_millis()
            );
            return Ok(());
        }
    };

    let http_status = response.status().as_u16() as i64;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !response.status().is_success() {
        mark_failed(&state.db, &url, http_status, &content_type, "http error").await?;
        info!(
            "ingest end: {} status=failed reason=http_error http_status={} elapsed_ms={}",
            url,
            http_status,
            start.elapsed().as_millis()
        );
        return Ok(());
    }

    if !content_type.starts_with("text/html") {
        mark_failed(
            &state.db,
            &url,
            http_status,
            &content_type,
            "unsupported content type",
        )
        .await?;
        info!(
            "ingest end: {} status=failed reason=unsupported_content_type content_type={} elapsed_ms={}",
            url,
            content_type,
            start.elapsed().as_millis()
        );
        return Ok(());
    }

    let html = match response.text().await {
        Ok(html) => html,
        Err(err) => {
            mark_failed(&state.db, &url, http_status, &content_type, &err.to_string()).await?;
            info!(
                "ingest end: {} status=failed reason=read_body_error error={} elapsed_ms={}",
                url,
                err,
                start.elapsed().as_millis()
            );
            return Ok(());
        }
    };
    let (title, body_text) = extract_text(&html);
    let cleaned = clean_text(&body_text);
    let excerpt = make_excerpt(&cleaned, 280);

    if let Err(err) = index_document(&state, &url, &title, &cleaned, &excerpt).await {
        mark_failed(&state.db, &url, http_status, &content_type, &err.to_string()).await?;
        info!(
            "ingest end: {} status=failed reason=index_error error={} elapsed_ms={}",
            url,
            err,
            start.elapsed().as_millis()
        );
        return Ok(());
    }

    let now = now_rfc3339();
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
    .execute(&state.db)
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

async fn index_document(
    state: &AppState,
    url: &str,
    title: &Option<String>,
    body: &str,
    excerpt: &Option<String>,
) -> anyhow::Result<()> {
    let mut writer = state.writer.lock().await;

    writer.delete_term(Term::from_field_text(state.fields.url, url));

    let fetched_at = OffsetDateTime::now_utc().unix_timestamp();
    let doc = doc!(
        state.fields.url => url,
        state.fields.title => title.clone().unwrap_or_default(),
        state.fields.body => body,
        state.fields.excerpt => excerpt.clone().unwrap_or_default(),
        state.fields.fetched_at => fetched_at,
    );

    writer.add_document(doc)?;
    writer.commit()?;
    state.reader.reload()?;
    Ok(())
}

async fn mark_failed(
    db: &SqlitePool,
    url: &str,
    http_status: i64,
    content_type: &str,
    error: &str,
) -> anyhow::Result<()> {
    let now = now_rfc3339();
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
    .execute(db)
    .await?;
    Ok(())
}

fn extract_text(html: &str) -> (Option<String>, String) {
    let document = Html::parse_document(html);
    let title = extract_title(&document);

    let body_text = html2text::from_read(html.as_bytes(), 80);
    (title, body_text)
}

fn extract_title(document: &Html) -> Option<String> {
    let og_title_selector = Selector::parse(r#"meta[property="og:title"]"#).unwrap();
    let twitter_title_selector = Selector::parse(r#"meta[name="twitter:title"]"#).unwrap();
    let h1_selector = Selector::parse("h1").unwrap();
    let title_selector = Selector::parse("title").unwrap();

    let candidates = [
        select_meta_content(document, &og_title_selector),
        select_meta_content(document, &twitter_title_selector),
        select_text(document, &h1_selector),
        select_text(document, &title_selector).and_then(|t| trim_site_suffix(&t)),
    ];

    candidates.into_iter().flatten().next()
}

fn select_meta_content(document: &Html, selector: &Selector) -> Option<String> {
    document
        .select(selector)
        .next()
        .and_then(|node| node.value().attr("content"))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn select_text(document: &Html, selector: &Selector) -> Option<String> {
    document
        .select(selector)
        .next()
        .map(|node| node.text().collect::<Vec<_>>().join(" "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

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

fn make_excerpt(text: &str, max_len: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let mut excerpt = text.chars().take(max_len).collect::<String>();
    if text.chars().count() > max_len {
        excerpt.push('…');
    }
    Some(excerpt)
}

fn normalize_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut url = Url::parse(trimmed).ok()?;
    url.set_fragment(None);
    Some(url.to_string())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
