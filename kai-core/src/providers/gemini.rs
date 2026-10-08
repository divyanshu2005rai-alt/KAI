// ============================================================
// providers/gemini.rs — Google Gemini API provider with quota fallback
//
// Implements the AiProvider trait for Google's Gemini API.
// Automatically falls back across models (gemini-2.5-flash, gemini-2.5-flash-lite, gemini-2.5-pro)
// if a single free-tier model hits its daily quota (HTTP 429).
// ============================================================

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{AiProvider, CompletionRequest, CompletionResponse, ProviderError, TokenUsage};

// ── Request structures ──────────────────────────────────────

#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
}

#[derive(Debug, Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

// ── Response structures ─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    error: Option<GeminiError>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiError {
    code: u16,
    message: String,
}

// ── Provider implementation ─────────────────────────────────

pub struct GeminiProvider {
    client: Client,
    api_key: String,
    primary_model: String,
}

impl GeminiProvider {
    pub fn new(api_key: String, primary_model: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("Failed to build HTTP client"),
            api_key,
            primary_model,
        }
    }
}

#[async_trait]
impl AiProvider for GeminiProvider {
    fn name(&self) -> &str {
        "Google Gemini"
    }

    fn model(&self) -> &str {
        &self.primary_model
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        // Models to try in sequence if one hits free quota limit (429)
        let candidate_models = vec![
            self.primary_model.as_str(),
            "gemini-2.5-flash-lite",
            "gemini-2.5-flash",
            "gemini-2.5-pro",
        ];

        let mut last_error = None;
        let history = request.history.clone();
        let user_prompt = request.user_prompt.clone();

        for model in candidate_models {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model, self.api_key
            );

            let mut contents = history
                .clone()
                .into_iter()
                .map(|message| GeminiContent {
                    role: if message.role == "assistant" {
                        "model".to_string()
                    } else {
                        "user".to_string()
                    },
                    parts: vec![GeminiPart {
                        text: message.content,
                    }],
                })
                .collect::<Vec<_>>();
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: user_prompt.clone(),
                }],
            });

            let body = GeminiRequest {
                system_instruction: Some(GeminiSystemInstruction {
                    parts: vec![GeminiPart {
                        text: request.system_prompt.clone(),
                    }],
                }),
                contents,
            };

            debug!("POST Gemini request with model {}", model);

            let response = match self
                .client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(ProviderError::Http(e));
                    continue;
                }
            };

            let status = response.status();
            info!("Gemini response status for {}: {}", model, status);

            if status.as_u16() == 429 {
                warn!(
                    "Model {} hit daily free quota limit (429) — trying fallback model...",
                    model
                );
                last_error = Some(ProviderError::RateLimit);
                continue; // Try next model!
            }

            if !status.is_success() {
                let status_u16 = status.as_u16();
                let error_text = response.text().await.unwrap_or_default();
                let lower_error = error_text.to_lowercase();
                if status_u16 == 413
                    || lower_error.contains("context length")
                    || lower_error.contains("too many tokens")
                    || lower_error.contains("maximum context")
                    || lower_error.contains("prompt is too long")
                {
                    return Err(ProviderError::InputTooLarge);
                }
                last_error = Some(ProviderError::Api {
                    status: status_u16,
                    message: error_text,
                });
                continue;
            }

            let gemini_resp: GeminiResponse = match response.json().await {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(ProviderError::Parse(e.to_string()));
                    continue;
                }
            };

            if let Some(err) = gemini_resp.error {
                last_error = Some(ProviderError::Api {
                    status: err.code,
                    message: err.message,
                });
                continue;
            }

            let text = match gemini_resp
                .candidates
                .and_then(|c| c.into_iter().next())
                .and_then(|c| c.content)
                .and_then(|c| c.parts.into_iter().next())
                .map(|p| p.text)
            {
                Some(t) => t,
                None => {
                    last_error = Some(ProviderError::NoContent);
                    continue;
                }
            };

            info!("✓ Gemini request succeeded with model {}", model);
            return Ok(CompletionResponse {
                text,
                usage: TokenUsage::default(),
                model: model.to_string(),
            });
        }

        Err(last_error.unwrap_or(ProviderError::NoContent))
    }
}
