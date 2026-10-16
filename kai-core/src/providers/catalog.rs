//! Live model discovery for the provider endpoints KAI can call.

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::{config::Config, credentials};

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub input_limit: Option<u64>,
    pub output_limit: Option<u64>,
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityStatus {
    Supported,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelCapabilities {
    /// Whether the provider catalog confirms KAI's current text chat request.
    pub text_chat: CapabilityStatus,
    /// Image input is reserved for future KAI vision requests.
    pub image_input: CapabilityStatus,
}

#[derive(Deserialize)]
struct OpenAiModelList {
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
    active: Option<bool>,
    context_window: Option<u64>,
    max_completion_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct GeminiModelList {
    models: Vec<GeminiModel>,
    #[serde(default, rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiModel {
    name: String,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
    input_token_limit: Option<u64>,
    output_token_limit: Option<u64>,
}

pub async fn list(config: &Config, provider: &str) -> Result<Vec<ModelInfo>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()?;
    let provider = provider.to_ascii_lowercase();
    let models: Vec<ModelInfo> = match provider.as_str() {
        "groq" => {
            let key = std::env::var("GROQ_API_KEY")
                .ok()
                .or_else(|| credentials::read_secret("KAI/GROQ_API_KEY"))
                .context("Save a Groq key before refreshing its model list")?;
            let response = client
                .get("https://api.groq.com/openai/v1/models")
                .bearer_auth(key)
                .send()
                .await?
                .error_for_status()?;
            response
                .json::<OpenAiModelList>()
                .await?
                .data
                .into_iter()
                .filter(|model| {
                    model.active != Some(false)
                        && text_chat_status(&model.id) != CapabilityStatus::Unsupported
                })
                .map(|model| ModelInfo {
                    capabilities: ModelCapabilities {
                        text_chat: text_chat_status(&model.id),
                        image_input: CapabilityStatus::Unknown,
                    },
                    id: model.id,
                    input_limit: model.context_window,
                    output_limit: model.max_completion_tokens,
                })
                .collect()
        }
        "gemini" => {
            let key = std::env::var("GEMINI_API_KEY")
                .ok()
                .or_else(|| credentials::read_secret("KAI/GEMINI_API_KEY"))
                .context("Save a Gemini key before refreshing its model list")?;
            let mut models = Vec::new();
            let mut page_token: Option<String> = None;
            let mut seen_tokens = std::collections::HashSet::new();
            loop {
                let mut request = client
                    .get("https://generativelanguage.googleapis.com/v1beta/models")
                    .query(&[("pageSize", "1000")])
                    .header("x-goog-api-key", &key);
                if let Some(token) = page_token.as_deref() {
                    request = request.query(&[("pageToken", token)]);
                }
                let page = request
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<GeminiModelList>()
                    .await?;
                models.extend(page.models);
                let Some(next_token) = page
                    .next_page_token
                    .filter(|token| !token.trim().is_empty())
                else {
                    break;
                };
                if !seen_tokens.insert(next_token.clone()) {
                    bail!("Gemini model catalog repeated a page token");
                }
                page_token = Some(next_token);
            }

            models
                .into_iter()
                .filter(|model| supports_text_chat(&model.supported_generation_methods))
                .map(|model| ModelInfo {
                    capabilities: ModelCapabilities {
                        text_chat: CapabilityStatus::Supported,
                        // Gemini's model list reports generation methods and token limits, not
                        // whether a model accepts image parts in KAI's future request shape.
                        image_input: CapabilityStatus::Unknown,
                    },
                    id: model
                        .name
                        .strip_prefix("models/")
                        .unwrap_or(&model.name)
                        .to_string(),
                    input_limit: model.input_token_limit,
                    output_limit: model.output_token_limit,
                })
                .collect()
        }
        "openai-compatible" => {
            let provider_config = config
                .providers
                .openai_compatible
                .as_ref()
                .context("Configure an OpenAI-compatible base URL before refreshing models")?;
            let key = provider_config
                .env_key
                .as_deref()
                .and_then(|name| std::env::var(name).ok())
                .or_else(|| std::env::var("KAI_OPENAI_COMPATIBLE_API_KEY").ok())
                .or_else(|| credentials::read_secret("KAI/OPENAI_COMPATIBLE_API_KEY"));
            let endpoint = format!("{}/models", provider_config.base_url.trim_end_matches('/'));
            let mut request = client.get(endpoint);
            if let Some(key) = key {
                request = request.bearer_auth(key);
            }
            request
                .send()
                .await?
                .error_for_status()?
                .json::<OpenAiModelList>()
                .await?
                .data
                .into_iter()
                .filter(|model| {
                    model.active != Some(false)
                        && text_chat_status(&model.id) != CapabilityStatus::Unsupported
                })
                .map(|model| ModelInfo {
                    capabilities: ModelCapabilities {
                        text_chat: text_chat_status(&model.id),
                        image_input: CapabilityStatus::Unknown,
                    },
                    id: model.id,
                    input_limit: model.context_window,
                    output_limit: model.max_completion_tokens,
                })
                .collect()
        }
        _ => bail!("choose Groq, Gemini, or OpenAI-compatible to refresh model IDs"),
    };

    if models.is_empty() {
        bail!("the provider returned no models supported by KAI's text request path");
    }
    Ok(models)
}

fn supports_text_chat(methods: &[String]) -> bool {
    methods.iter().any(|method| method == "generateContent")
}

fn text_chat_status(id: &str) -> CapabilityStatus {
    let id = id.to_ascii_lowercase();
    if [
        "whisper",
        "transcribe",
        "tts",
        "orpheus",
        "embedding",
        "embed-",
    ]
    .iter()
    .any(|unsupported| id.contains(unsupported))
    {
        CapabilityStatus::Unsupported
    } else {
        // OpenAI-compatible model catalogs do not consistently publish chat
        // capabilities; the UI's Test model action probes the actual endpoint.
        CapabilityStatus::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::{supports_text_chat, text_chat_status, CapabilityStatus, GeminiModelList};

    #[test]
    fn gemini_catalog_requires_the_text_generation_method() {
        assert!(supports_text_chat(&["generateContent".to_string()]));
        assert!(!supports_text_chat(&["embedContent".to_string()]));
        assert!(!supports_text_chat(&[]));
    }

    #[test]
    fn model_ids_separate_known_non_text_from_unverified_models() {
        assert_eq!(
            text_chat_status("whisper-large-v3"),
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            text_chat_status("qwen/qwen3.8-27b"),
            CapabilityStatus::Unknown
        );
    }

    #[test]
    fn gemini_catalog_reads_the_next_page_token() {
        let page: GeminiModelList =
            serde_json::from_str(r#"{"models":[],"nextPageToken":"next-page"}"#)
                .expect("Gemini page response should deserialize");
        assert_eq!(page.next_page_token.as_deref(), Some("next-page"));
    }
}
