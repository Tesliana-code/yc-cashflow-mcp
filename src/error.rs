use serde::Serialize;
use serde_json::{Value, json};

/// Stable error categories surfaced to MCP clients in `structuredContent.error.category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    RateLimited,
    NotFound,
    SourceUnavailable,
    SchemaDrift,
    InvalidArguments,
    ResponseTooLarge,
    UrlNotAllowed,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{category:?}: {message}")]
pub struct SourceError {
    pub category: ErrorCategory,
    pub message: String,
}

impl SourceError {
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::InvalidArguments, message)
    }

    /// Whether a retry could plausibly succeed (transient network / 5xx failures only).
    pub fn is_retryable(&self) -> bool {
        self.category == ErrorCategory::SourceUnavailable
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ok": false,
            "error": { "category": self.category, "message": self.message }
        })
    }
}
