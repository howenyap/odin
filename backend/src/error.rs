use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tantivy::TantivyError;
use tracing::error;

#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    message: String,
    source: Option<anyhow::Error>,
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            source: None,
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
            source: None,
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "internal error".to_string(),
            source: Some(value),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "database error".to_string(),
            source: Some(value.into()),
        }
    }
}

impl From<TantivyError> for AppError {
    fn from(value: TantivyError) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "search index error".to_string(),
            source: Some(value.into()),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Some(source) = self.source {
            error!("{:?}", source);
        }
        (self.status, self.message).into_response()
    }
}
