// ============================================================
// providers/openai.rs — OpenAI API provider implementation
//
// Implements the AiProvider trait for OpenAI's chat completions API.
// Supports gpt-4o, gpt-4o-mini, gpt-4-turbo, and any compatible model.
// ============================================================

use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
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

#[derive(Debug, Deserialize)]
struct StreamResponse {
    model: Option<String>,
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
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

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: request.system_prompt,
        }];
        messages.extend(request.history.into_iter().map(|message| ChatMessage {
            role: message.role,
            content: message.content,
        }));
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: request.user_prompt,
        });

        let body = ChatRequest {
            model: self.model.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: 0.7,
            stream: None,
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

            // Providers commonly report an oversized context as 400, 413,
            // or occasionally 500. Convert recognizable messages into a
            // stable error so the UI can explain the actual problem.
            let lower_message = message.to_lowercase();
            if status_u16 == 413
                || lower_message.contains("context length")
                || lower_message.contains("too many tokens")
                || lower_message.contains("maximum context")
                || lower_message.contains("prompt is too long")
            {
                return Err(ProviderError::InputTooLarge);
            }

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

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: request.system_prompt,
        }];
        messages.extend(request.history.into_iter().map(|message| ChatMessage {
            role: message.role,
            content: message.content,
        }));
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: request.user_prompt,
        });

        let body = ChatRequest {
            model: self.model.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: 0.7,
            stream: Some(true),
        };

        debug!("POST {} with streaming model {}", url, self.model);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let status_u16 = status.as_u16();
            let error_text = response.text().await.unwrap_or_default();
            if status_u16 == 401 {
                return Err(ProviderError::Auth);
            }
            if status_u16 == 429 {
                return Err(ProviderError::RateLimit);
            }
            return Err(ProviderError::Api {
                status: status_u16,
                message: error_text,
            });
        }

        let mut bytes = response.bytes_stream();
        let mut pending = String::new();
        let mut text = String::new();
        let mut response_model = self.model.clone();

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk?;
            pending.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(newline) = pending.find('\n') {
                let line = pending[..newline].trim().to_string();
                pending.drain(..=newline);
                if let Some(payload) = line.strip_prefix("data:") {
                    let payload = payload.trim();
                    if payload == "[DONE]" || payload.is_empty() {
                        continue;
                    }
                    let event: StreamResponse = serde_json::from_str(payload)
                        .map_err(|error| ProviderError::Parse(error.to_string()))?;
                    if let Some(model) = event.model {
                        response_model = model;
                    }
                    for choice in event.choices {
                        if let Some(delta) = choice.delta.content {
                            text.push_str(&delta);
                        }
                    }
                }
            }
        }

        if let Some(payload) = pending.trim().strip_prefix("data:") {
            let payload = payload.trim();
            if payload != "[DONE]" && !payload.is_empty() {
                let event: StreamResponse = serde_json::from_str(payload)
                    .map_err(|error| ProviderError::Parse(error.to_string()))?;
                for choice in event.choices {
                    if let Some(delta) = choice.delta.content {
                        text.push_str(&delta);
                    }
                }
            }
        }

        if text.is_empty() {
            return Err(ProviderError::NoContent);
        }

        Ok(CompletionResponse {
            text,
            usage: TokenUsage::default(),
            model: response_model,
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}
