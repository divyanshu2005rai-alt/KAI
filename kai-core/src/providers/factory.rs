//! Provider construction and selection.
//!
//! The rest of KAI should depend on `AiProvider`, not on vendor-specific
//! implementations. This module is the single boundary where configuration is
//! translated into a concrete provider. Future Qwen, local, and vision
//! providers can be added here without changing trigger or injection logic.

use std::sync::Arc;

use anyhow::{bail, Result};
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

#[derive(Debug, PartialEq, Eq)]
enum RouteSelection {
    Primary,
    Fallback,
    Unavailable,
}

fn route_selection(
    primary_available: bool,
    fallback_enabled: bool,
    fallback_available: bool,
) -> RouteSelection {
    if primary_available {
        RouteSelection::Primary
    } else if fallback_enabled && fallback_available {
        RouteSelection::Fallback
    } else {
        RouteSelection::Unavailable
    }
}

#[async_trait]
impl AiProvider for FailoverProvider {
    fn name(&self) -> &str {
        "Provider failover"
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
    build_for(config, None, None)
}

/// Build a provider for one request, applying an optional trigger route.
/// Environment overrides remain authoritative for command-line deployments.
pub fn build_for(
    config: &Config,
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<Arc<dyn AiProvider>> {
    build_selected(config, provider_override, model_override, true)
}

/// Build the selected provider/model for the settings UI's compatibility check.
/// Environment routing and cross-provider failover are bypassed. Gemini may
/// still retry its own quota fallback models; the caller must report the model
/// returned by the provider. Credential environment variables still work.
pub fn build_exact(config: &Config, provider: &str, model: &str) -> Result<Arc<dyn AiProvider>> {
    let mut exact_config = config.clone();
    match provider {
        "groq"
            if std::env::var("GROQ_API_KEY").is_err()
                && credentials::read_secret("KAI/GROQ_API_KEY").is_none() =>
        {
            bail!("No Groq API key is saved")
        }
        "gemini"
            if std::env::var("GEMINI_API_KEY").is_err()
                && credentials::read_secret("KAI/GEMINI_API_KEY").is_none() =>
        {
            bail!("No Gemini API key is saved")
        }
        "openai-compatible" if exact_config.providers.openai_compatible.is_none() => {
            bail!("Configure the OpenAI-compatible base URL before testing")
        }
        "groq" | "gemini" | "openai-compatible" => {}
        _ => bail!("unsupported provider for model test: {provider}"),
    }
    if model.trim().is_empty() {
        bail!("model ID cannot be empty")
    }
    exact_config.routing.fallback = false;
    build_selected(&exact_config, Some(provider), Some(model), false)
}

fn build_selected(
    config: &Config,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    respect_environment_routing: bool,
) -> Result<Arc<dyn AiProvider>> {
    let env_provider = if respect_environment_routing {
        std::env::var("KAI_PROVIDER").ok()
    } else {
        None
    };
    let model_override = if env_provider.is_none() {
        model_override
    } else {
        None
    };
    let active = env_provider
        .or_else(|| provider_override.map(str::to_string))
        .or_else(|| config.providers.active_provider.clone())
        .unwrap_or_else(|| "auto".to_string())
        .to_ascii_lowercase();
    let strategy = config.routing.strategy.to_ascii_lowercase();

    let groq_key = std::env::var("GROQ_API_KEY")
        .ok()
        .or_else(|| credentials::read_secret("KAI/GROQ_API_KEY"));
    let gemini_key = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| credentials::read_secret("KAI/GEMINI_API_KEY"));
    let prefer_local = active == "local"
        || active == "openai-compatible"
        || (active == "auto"
            && (strategy == "local" || (strategy == "auto" && config.routing.local_first)));
    let local_is_primary = prefer_local && config.providers.openai_compatible.is_some();
    let groq_is_primary = !local_is_primary
        && (active == "groq" || ((active == "auto" || active == "cloud") && groq_key.is_some()));
    let gemini_is_primary = !local_is_primary
        && !groq_is_primary
        && (active == "gemini"
            || ((active == "auto" || active == "cloud") && gemini_key.is_some()));

    let groq_provider = if (active == "auto"
        || active == "cloud"
        || active == "groq"
        || (config.routing.fallback
            && (active == "gemini" || active == "local" || active == "openai-compatible")))
        && groq_key.is_some()
    {
        let model = (groq_is_primary)
            .then(|| {
                respect_environment_routing
                    .then(|| std::env::var("KAI_MODEL").ok())
                    .flatten()
            })
            .flatten()
            .or_else(|| {
                respect_environment_routing
                    .then(|| std::env::var("GROQ_MODEL").ok())
                    .flatten()
            })
            .or_else(|| {
                groq_is_primary
                    .then(|| model_override.map(str::to_string))
                    .flatten()
            })
            .or_else(|| {
                config
                    .providers
                    .groq
                    .as_ref()
                    .and_then(|provider| provider.model.clone())
            })
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

    let gemini_provider = if (active == "auto"
        || active == "cloud"
        || active == "gemini"
        || (config.routing.fallback
            && (active == "groq" || active == "local" || active == "openai-compatible")))
        && gemini_key.is_some()
    {
        let model = gemini_is_primary
            .then(|| {
                respect_environment_routing
                    .then(|| std::env::var("KAI_MODEL").ok())
                    .flatten()
            })
            .flatten()
            .or_else(|| {
                respect_environment_routing
                    .then(|| std::env::var("GEMINI_MODEL").ok())
                    .flatten()
            })
            .or_else(|| {
                gemini_is_primary
                    .then(|| model_override.map(str::to_string))
                    .flatten()
            })
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

    let cloud_provider = if active == "gemini" {
        if let Some(primary) = gemini_provider.clone() {
            if let Some(fallback) = groq_provider.clone() {
                Some(Arc::new(FailoverProvider { primary, fallback }) as Arc<dyn AiProvider>)
            } else {
                Some(primary)
            }
        } else {
            groq_provider.clone()
        }
    } else {
        if let Some(primary) = groq_provider.clone() {
            if let Some(fallback) = gemini_provider.clone() {
                Some(Arc::new(FailoverProvider { primary, fallback }) as Arc<dyn AiProvider>)
            } else {
                Some(primary)
            }
        } else {
            gemini_provider.clone()
        }
    };

    let local_provider = config.providers.openai_compatible.as_ref().map(|provider| {
        let key = provider
            .env_key
            .as_deref()
            .and_then(|name| std::env::var(name).ok())
            .or_else(|| std::env::var("KAI_OPENAI_COMPATIBLE_API_KEY").ok())
            .or_else(|| credentials::read_secret("KAI/OPENAI_COMPATIBLE_API_KEY"))
            .unwrap_or_default();
        let model = local_is_primary
            .then(|| {
                respect_environment_routing
                    .then(|| std::env::var("KAI_MODEL").ok())
                    .flatten()
            })
            .flatten()
            .or_else(|| {
                local_is_primary
                    .then(|| model_override.map(str::to_string))
                    .flatten()
            })
            .unwrap_or_else(|| provider.default_model.clone());
        let base_url = respect_environment_routing
            .then(|| std::env::var("KAI_BASE_URL").ok())
            .flatten()
            .unwrap_or_else(|| provider.base_url.clone());
        info!(
            "Using OpenAI-compatible provider at {} (model: {})",
            base_url, model
        );
        Arc::new(OpenAiProvider::with_base_url(key, model, base_url)) as Arc<dyn AiProvider>
    });

    if active == "openai-compatible" || active == "local" {
        if let Some(primary) = local_provider {
            if config.routing.fallback {
                if let Some(fallback) = cloud_provider {
                    return Ok(Arc::new(FailoverProvider { primary, fallback }));
                }
            }
            return Ok(primary);
        }
        if config.routing.fallback {
            if let Some(fallback) = cloud_provider {
                return Ok(fallback);
            }
        }
        bail!("providers.openai_compatible is not configured");
    }

    if active != "auto" && active != "cloud" && active != "groq" && active != "gemini" {
        bail!("unsupported providers.active_provider: {}", active);
    }

    let (primary, fallback) = if matches!(active.as_str(), "groq" | "gemini" | "cloud") {
        (cloud_provider, local_provider)
    } else if prefer_local {
        (local_provider, cloud_provider)
    } else {
        (cloud_provider, local_provider)
    };

    match route_selection(
        primary.is_some(),
        config.routing.fallback,
        fallback.is_some(),
    ) {
        RouteSelection::Primary => {
            let primary = primary.ok_or_else(|| anyhow::anyhow!("primary route disappeared"))?;
            if config.routing.fallback {
                if let Some(fallback) = fallback {
                    return Ok(Arc::new(FailoverProvider { primary, fallback }));
                }
            }
            Ok(primary)
        }
        RouteSelection::Fallback => {
            fallback.ok_or_else(|| anyhow::anyhow!("fallback route disappeared"))
        }
        RouteSelection::Unavailable if !config.routing.fallback => bail!(
            "preferred provider route is unavailable; configure its credentials or enable fallback"
        ),
        RouteSelection::Unavailable => bail!("no configured AI provider credentials found"),
    }
}

#[cfg(test)]
mod tests {
    use super::{route_selection, FailoverProvider, RouteSelection};
    use crate::providers::{AiProvider, CompletionRequest, CompletionResponse, ProviderError};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct StubProvider {
        name: &'static str,
        fails: bool,
    }

    #[async_trait]
    impl AiProvider for StubProvider {
        fn name(&self) -> &str {
            self.name
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            if self.fails {
                Err(ProviderError::NoContent)
            } else {
                Ok(CompletionResponse {
                    text: "ok".to_string(),
                    model: self.name.to_string(),
                })
            }
        }
    }

    #[test]
    fn preferred_route_is_kept_when_available() {
        assert_eq!(route_selection(true, true, true), RouteSelection::Primary);
        assert_eq!(route_selection(true, false, true), RouteSelection::Primary);
    }

    #[test]
    fn fallback_is_used_only_when_enabled_and_available() {
        assert_eq!(route_selection(false, true, true), RouteSelection::Fallback);
        assert_eq!(
            route_selection(false, false, true),
            RouteSelection::Unavailable
        );
        assert_eq!(
            route_selection(false, true, false),
            RouteSelection::Unavailable
        );
    }

    #[tokio::test]
    async fn failover_calls_the_secondary_provider_after_primary_failure() {
        let provider = FailoverProvider {
            primary: Arc::new(StubProvider {
                name: "primary",
                fails: true,
            }),
            fallback: Arc::new(StubProvider {
                name: "fallback",
                fails: false,
            }),
        };
        let response = provider
            .complete(CompletionRequest {
                system_prompt: "test".to_string(),
                user_prompt: "test".to_string(),
                history: Vec::new(),
                max_tokens: Some(8),
                stream: false,
            })
            .await
            .unwrap();
        assert_eq!(response.model, "fallback");
    }
}
