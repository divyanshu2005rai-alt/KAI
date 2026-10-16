// ============================================================
// providers/mod.rs — AI Provider abstraction layer
//
// Defines the AiProvider trait and shared request/response types.
// All AI providers (OpenAI, Anthropic, Gemini, local) implement
// this trait, enabling easy swapping and multi-provider support.
// ============================================================

pub mod catalog;
pub mod factory;
pub mod gemini;
pub mod openai;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A request to an AI completion provider.
#[derive(Debug, Clone, Serialize)]
pub struct CompletionRequest {
    /// System-level instructions (persona, constraints, format rules)
    pub system_prompt: String,
    /// The user's actual prompt/query
    pub user_prompt: String,
    /// Recent conversation turns, bounded by application policy.
    #[serde(default)]
    pub history: Vec<ConversationMessage>,
    /// Maximum tokens in the response (None = provider default)
    pub max_tokens: Option<u32>,
    /// Whether the provider should use its streaming transport when available.
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

/// A completed AI response.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    /// The generated text (already extracted from the provider's JSON structure)
    pub text: String,
    /// Which model actually generated the response
    pub model: String,
}

/// Errors that can occur when calling an AI provider.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("Response parse error: {0}")]
    Parse(String),

    #[error("Authentication error — check your API key")]
    Auth,

    #[error("Rate limited — too many requests")]
    RateLimit,

    #[error("Input is too large for the selected model")]
    InputTooLarge,

    #[error("No content in response")]
    NoContent,
}

/// The core trait all AI providers must implement.
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Human-readable provider name (e.g. "OpenAI", "Anthropic")
    fn name(&self) -> &str;

    /// Send a completion request and await the full response.
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError>;

    /// Complete using a streaming transport when the provider supports it.
    /// Providers without streaming support safely fall back to `complete`.
    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        self.complete(request).await
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}
