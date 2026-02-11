use std::sync::Arc;

use axum::http::HeaderMap;
use reqwest::header::AUTHORIZATION;

use crate::errors::AppError;
use crate::types::Dependencies;

#[derive(Clone)]
pub struct AuthService {
    deps: Arc<Dependencies>,
}

impl AuthService {
    pub fn new(deps: Arc<Dependencies>) -> Self {
        Self { deps }
    }

    pub fn authorize(&self, headers: &HeaderMap) -> Result<(), AppError> {
        let Some(raw_header) = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
        else {
            return Err(AppError::unauthorized("missing authorization header"));
        };

        let token = raw_header
            .strip_prefix("Bearer ")
            .map(str::trim)
            .unwrap_or_default();

        if token.is_empty() {
            return Err(AppError::unauthorized("missing admin token"));
        }

        if token != self.deps.admin_token {
            return Err(AppError::unauthorized("invalid admin token"));
        }

        Ok(())
    }
}
