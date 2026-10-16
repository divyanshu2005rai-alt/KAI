//! Application settings service.
//!
//! UI adapters should use this service rather than reading or writing TOML
//! directly. Validation and atomic persistence stay in one place.

use anyhow::Result;
use std::io::{self, Write};

use crate::config::Config;

pub struct SettingsService;

impl SettingsService {
    pub fn load() -> Result<Config> {
        Config::load()
    }

    pub fn save(config: &Config) -> Result<()> {
        config.save()
    }

    pub fn path() -> std::path::PathBuf {
        Config::config_file()
    }

    /// Minimal interactive settings editor used until the graphical settings
    /// window is available. It still goes through the same validated save API.
    pub fn interactive() -> Result<()> {
        let mut config = Self::load().unwrap_or_default();
        println!("KAI settings");
        println!("Configuration file: {:?}", Self::path());

        let prefix = prompt("Trigger prefix", &config.general.trigger_prefix)?;
        if !prefix.is_empty() {
            config.general.trigger_prefix = prefix;
        }

        let timeout = prompt(
            "Timeout seconds",
            &config.general.timeout_seconds.to_string(),
        )?;
        if !timeout.is_empty() {
            config.general.timeout_seconds = timeout.parse()?;
        }

        let max_prompt = prompt(
            "Maximum prompt characters",
            &config.general.max_prompt_chars.to_string(),
        )?;
        if !max_prompt.is_empty() {
            config.general.max_prompt_chars = max_prompt.parse()?;
        }

        let context_turns = prompt(
            "Context turns retained in memory",
            &config.general.context_turns.to_string(),
        )?;
        if !context_turns.is_empty() {
            config.general.context_turns = context_turns.parse()?;
        }

        let include_selected_text = prompt(
            "Include selected text in requests (true/false)",
            &config.general.include_selected_text.to_string(),
        )?;
        if !include_selected_text.is_empty() {
            config.general.include_selected_text = include_selected_text.parse()?;
        }

        let stream_responses = prompt(
            "Stream provider responses (true/false)",
            &config.general.stream_responses.to_string(),
        )?;
        if !stream_responses.is_empty() {
            config.general.stream_responses = stream_responses.parse()?;
        }

        let routing_mode = prompt(
            "Routing mode (automatic/ manual/ trigger-based)",
            &config.routing.mode,
        )?;
        if !routing_mode.is_empty() {
            config.routing.mode = routing_mode.to_ascii_lowercase();
        }

        let routing_strategy = prompt(
            "Routing strategy (auto/cloud/local)",
            &config.routing.strategy,
        )?;
        if !routing_strategy.is_empty() {
            config.routing.strategy = routing_strategy;
        }

        let local_first = prompt(
            "Prefer local provider in auto mode (true/false)",
            &config.routing.local_first.to_string(),
        )?;
        if !local_first.is_empty() {
            config.routing.local_first = local_first.parse()?;
        }

        if config.routing.mode == "automatic" {
            config.providers.active_provider = Some("auto".to_string());
        } else {
            let provider = prompt(
                "Provider (auto/groq/gemini/openai-compatible)",
                config
                    .providers
                    .active_provider
                    .as_deref()
                    .unwrap_or("auto"),
            )?;
            if !provider.is_empty() {
                config.providers.active_provider = Some(provider);
            }
        }

        let groq_model = prompt(
            "Groq model",
            config
                .providers
                .groq
                .as_ref()
                .and_then(|provider| provider.model.as_deref())
                .unwrap_or("openai/gpt-oss-120b"),
        )?;
        if !groq_model.is_empty() {
            config
                .providers
                .groq
                .get_or_insert_with(|| crate::config::GroqConfig { model: None })
                .model = Some(groq_model);
        }

        let gemini_model = prompt(
            "Gemini model",
            config
                .providers
                .gemini
                .as_ref()
                .and_then(|provider| provider.default_model.as_deref())
                .unwrap_or("gemini-2.5-flash"),
        )?;
        if !gemini_model.is_empty() {
            config
                .providers
                .gemini
                .get_or_insert_with(|| crate::config::GeminiConfig {
                    default_model: None,
                    legacy_api_key: None,
                })
                .default_model = Some(gemini_model);
        }

        let local_base_url = prompt(
            "Local OpenAI-compatible base URL",
            config
                .providers
                .openai_compatible
                .as_ref()
                .map(|provider| provider.base_url.as_str())
                .unwrap_or("http://127.0.0.1:11434/v1"),
        )?;
        let local_model = prompt(
            "Local OpenAI-compatible model",
            config
                .providers
                .openai_compatible
                .as_ref()
                .map(|provider| provider.default_model.as_str())
                .unwrap_or("llama3.2"),
        )?;
        if !local_base_url.is_empty() || !local_model.is_empty() {
            let provider = config.providers.openai_compatible.get_or_insert_with(|| {
                crate::config::OpenAiCompatibleConfig {
                    base_url: "http://127.0.0.1:11434/v1".to_string(),
                    default_model: "llama3.2".to_string(),
                    env_key: None,
                    legacy_api_key: None,
                }
            });
            if !local_base_url.is_empty() {
                provider.base_url = local_base_url;
            }
            if !local_model.is_empty() {
                provider.default_model = local_model;
            }
        }

        Self::save(&config)?;
        println!("Settings saved. Restart KAI to apply changes.");
        Ok(())
    }
}

fn prompt(label: &str, current: &str) -> Result<String> {
    print!("{} [{}]: ", label, current);
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_string())
}
