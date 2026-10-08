//! Provider construction and selection.
//!
//! The rest of KAI should depend on `AiProvider`, not on vendor-specific
//! implementations. This module is the single boundary where configuration is
//! translated into a concrete provider. Future Qwen, local, and vision
//! providers can be added here without changing trigger or injection logic.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use log::info;

use crate::config::Config;
use crate::credentials;

use async_trait::async_trait;

use super::{
    openai::OpenAiProvider, AiProvider, CompletionRequest, CompletionResponse, ProviderError,
};

/// Keeps retry/failover policy outside the trigger and UI layers.
struct FailoverProvider {
    primary: Arc<dyn AiProvider>,
    fallback: Arc<dyn AiProvider>,
}

#[async_trait]
impl AiProvider for FailoverProvider {
    fn name(&self) -> &str {
        "Provider failover"
    }

    fn model(&self) -> &str {
        self.primary.model()
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        match self.primary.complete(request.clone()).await {
            Ok(response) => Ok(response),
            Err(primary_error) => {
                log::warn!(
                    "Primary provider {} failed: {}; trying {}",
                    self.primary.name(),
                    primary_error,
                    self.fallback.name()
                );
                self.fallback.complete(request).await
            }
        }
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        match self.primary.complete_stream(request.clone()).await {
            Ok(response) => Ok(response),
            Err(primary_error) => {
                log::warn!(
                    "Streaming provider {} failed: {}; trying {}",
                    self.primary.name(),
                    primary_error,
                    self.fallback.name()
                );
                self.fallback.complete_stream(request).await
            }
        }
    }

    fn supports_streaming(&self) -> bool {
        self.primary.supports_streaming() || self.fallback.supports_streaming()
    }
}

/// Build the configured provider using environment variables first and config
/// values second. Environment variables are preferred so secrets stay out of
/// config files and source control.
pub fn build(config: &Config) -> Result<Arc<dyn AiProvider>> {
    let active = std::env::var("KAI_PROVIDER")
        .ok()
        .or_else(|| config.providers.active_provider.clone())
        .unwrap_or_else(|| "auto".to_string())
        .to_ascii_lowercase();
    let strategy = config.routing.strategy.to_ascii_lowercase();

    let groq_key = std::env::var("GROQ_API_KEY")
        .ok()
        .or_else(|| credentials::read_secret("KAI/GROQ_API_KEY"));
    let gemini_key = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| {
            config
                .providers
                .gemini
                .as_ref()
                .and_then(|provider| provider.api_key.clone())
        })
        .or_else(|| credentials::read_secret("KAI/GEMINI_API_KEY"));

    let groq_provider =
        if (active == "auto" || active == "cloud" || active == "groq") && groq_key.is_some() {
            let model = std::env::var("KAI_MODEL")
                .ok()
                .or_else(|| std::env::var("GROQ_MODEL").ok())
                .unwrap_or_else(|| "openai/gpt-oss-120b".to_string());
            info!(
                "Using Groq through OpenAI-compatible provider (model: {})",
                model
            );
            Some(Arc::new(OpenAiProvider::with_base_url(
                groq_key.expect("checked above"),
                model,
                "https://api.groq.com/openai/v1".to_string(),
            )) as Arc<dyn AiProvider>)
        } else {
            None
        };

    let gemini_provider =
        if (active == "auto" || active == "cloud" || active == "gemini") && gemini_key.is_some() {
            let model = std::env::var("KAI_MODEL")
                .ok()
                .or_else(|| {
                    config
                        .providers
                        .gemini
                        .as_ref()
                        .and_then(|provider| provider.default_model.clone())
                })
                .unwrap_or_else(|| "gemini-2.5-flash".to_string());
            info!("Using Google Gemini provider (model: {})", model);
            Some(Arc::new(super::gemini::GeminiProvider::new(
                gemini_key.expect("checked above"),
                model,
            )) as Arc<dyn AiProvider>)
        } else {
            None
        };

    let cloud_provider = if let Some(primary) = groq_provider {
        if let Some(fallback) = gemini_provider {
            Some(Arc::new(FailoverProvider { primary, fallback }) as Arc<dyn AiProvider>)
        } else {
            Some(primary)
        }
    } else {
        gemini_provider
    };

    let local_provider = config.providers.openai_compatible.as_ref().map(|provider| {
        let key = provider
            .env_key
            .as_deref()
            .and_then(|name| std::env::var(name).ok())
            .or_else(|| provider.api_key.clone())
            .unwrap_or_default();
        let model = std::env::var("KAI_MODEL")
            .ok()
            .unwrap_or_else(|| provider.default_model.clone());
        let base_url = std::env::var("KAI_BASE_URL")
            .ok()
            .unwrap_or_else(|| provider.base_url.clone());
        info!(
            "Using OpenAI-compatible provider at {} (model: {})",
            base_url, model
        );
        Arc::new(OpenAiProvider::with_base_url(key, model, base_url)) as Arc<dyn AiProvider>
    });

    if active == "openai-compatible" || active == "local" {
        return local_provider.context("providers.openai_compatible is not configured");
    }

    if active != "auto" && active != "cloud" && active != "groq" && active != "gemini" {
        bail!("unsupported providers.active_provider: {}", active);
    }

    let prefer_local = active == "local"
        || strategy == "local"
        || (strategy == "auto" && config.routing.local_first);
    let selected = if prefer_local {
        local_provider.clone().or(cloud_provider.clone())
    } else {
        cloud_provider.clone().or(local_provider.clone())
    };

    if config.routing.fallback {
        match (prefer_local, selected, local_provider, cloud_provider) {
            (true, Some(primary), Some(local), Some(cloud)) if Arc::ptr_eq(&primary, &local) => {
                return Ok(Arc::new(FailoverProvider {
                    primary,
                    fallback: cloud,
                }));
            }
            (false, Some(primary), Some(local), Some(cloud)) if Arc::ptr_eq(&primary, &cloud) => {
                return Ok(Arc::new(FailoverProvider {
                    primary,
                    fallback: local,
                }));
            }
            (_, Some(provider), _, _) => return Ok(provider),
            _ => {}
        }
    } else if let Some(provider) = selected {
        return Ok(provider);
    }

    bail!("no configured AI provider credentials found")
}
