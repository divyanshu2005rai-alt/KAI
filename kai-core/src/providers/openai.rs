// ============================================================
// providers/openai.rs — OpenAI API provider implementation
//
// Implements the AiProvider trait for OpenAI's chat completions API.
// Supports gpt-4o, gpt-4o-mini, gpt-4-turbo, and any compatible model.
// ============================================================

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{AiProvider, CompletionRequest, CompletionResponse, ProviderError, TokenUsage};

// ── Request structures ──────────────────────────────────────

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

// ── Response structures ─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChatResponse {
    id: String,
    model: String,
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
    code: Option<String>,
}

// ── Provider implementation ─────────────────────────────────

/// OpenAI chat completions provider.
pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("Failed to build HTTP client"),
            api_key,
            model,
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }

    /// Creates an OpenAI-compatible provider with a custom base URL.
    /// Useful for Ollama, LM Studio, Azure OpenAI, etc.
    pub fn with_base_url(api_key: String, model: String, base_url: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("Failed to build HTTP client"),
            api_key,
            model,
            base_url,
        }
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "OpenAI"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        let body = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: request.system_prompt,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: request.user_prompt,
                },
            ],
            max_tokens: request.max_tokens,
            temperature: 0.7,
        };

        debug!("POST {} with model {}", url, self.model);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        info!("OpenAI response status: {}", status);

        // Handle error responses
        if !status.is_success() {
            let status_u16 = status.as_u16();

            if status_u16 == 401 {
                return Err(ProviderError::Auth);
            }
            if status_u16 == 429 {
                return Err(ProviderError::RateLimit);
            }

            // Try to parse the error body
            let error_text = response.text().await.unwrap_or_default();
            let message = if let Ok(err_body) = serde_json::from_str::<ApiErrorBody>(&error_text) {
                err_body.error.message
            } else {
                error_text
            };

            return Err(ProviderError::Api {
                status: status_u16,
                message,
            });
        }

        // Parse successful response
        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        debug!("Response ID: {}", chat_response.id);

        let text = chat_response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or(ProviderError::NoContent)?;

        let usage = chat_response
            .usage
            .map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            })
            .unwrap_or_default();

        Ok(CompletionResponse {
            text,
            usage,
            model: chat_response.model,
        })
    }
}
