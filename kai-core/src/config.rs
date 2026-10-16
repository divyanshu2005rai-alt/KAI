// ============================================================
// config.rs — Configuration loading and defaults
//
// Loads KAI configuration from:
//   Windows:  %APPDATA%\kai\config.toml
//   macOS:    ~/Library/Application Support/kai/config.toml
//   Linux:    ~/.config/kai/config.toml
//
// Creates a default config file if none exists.
// ============================================================

use anyhow::bail;
use anyhow::{Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub routing: RoutingConfig,
    pub injection: InjectionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// The prefix that triggers KAI (default: "//")
    pub trigger_prefix: String,
    /// How to terminate a prompt — "alt" is the default mode in Phase 1
    pub terminator: String,
    /// Maximum seconds to wait for an AI response before giving up
    pub timeout_seconds: u64,
    /// Maximum prompt characters accepted before dispatch.
    #[serde(default = "default_max_prompt_chars")]
    pub max_prompt_chars: usize,
    /// Number of recent user/assistant turns retained in memory.
    #[serde(default = "default_context_turns")]
    pub context_turns: usize,
    /// Whether to include selected text in provider requests. Disabled by default
    /// because selection may contain passwords or other private data.
    #[serde(default)]
    pub include_selected_text: bool,
    /// Request streamed output when the selected provider supports it.
    /// KAI still injects only the completed response into the target app.
    #[serde(default)]
    pub stream_responses: bool,
}

fn default_max_prompt_chars() -> usize {
    12_000
}

fn default_context_turns() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub active_provider: Option<String>,
    #[serde(default)]
    pub groq: Option<GroqConfig>,
    #[serde(default)]
    pub gemini: Option<GeminiConfig>,
    #[serde(default)]
    pub openai_compatible: Option<OpenAiCompatibleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroqConfig {
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// "automatic", "manual", or "trigger-based".
    #[serde(default = "default_routing_mode")]
    pub mode: String,
    /// "auto", "cloud", or "local".
    #[serde(default = "default_routing_strategy")]
    pub strategy: String,
    /// Prefer the configured OpenAI-compatible endpoint in auto mode.
    #[serde(default)]
    pub local_first: bool,
    /// Try another configured route after a provider failure.
    #[serde(default = "default_routing_fallback")]
    pub fallback: bool,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            mode: default_routing_mode(),
            strategy: default_routing_strategy(),
            local_first: false,
            fallback: default_routing_fallback(),
        }
    }
}

fn default_routing_mode() -> String {
    "automatic".to_string()
}

fn default_routing_strategy() -> String {
    "auto".to_string()
}

fn default_routing_fallback() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    pub default_model: Option<String>,
    /// Read only to migrate older plaintext configs into Credential Manager.
    #[serde(rename = "api_key", default, skip_serializing)]
    pub(crate) legacy_api_key: Option<String>,
}

/// Configuration for OpenAI-compatible APIs such as Groq, Qwen-compatible
/// gateways, Ollama, and LM Studio. Keeping this generic avoids coupling the
/// application to one vendor when new providers are added later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub default_model: String,
    #[serde(default)]
    pub env_key: Option<String>,
    /// Read only to migrate older plaintext configs into Credential Manager.
    #[serde(rename = "api_key", default, skip_serializing)]
    pub(crate) legacy_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionConfig {
    /// Characters above this threshold use clipboard paste instead of keystroke simulation
    pub clipboard_threshold: usize,
    /// Delay between backspace keystrokes in milliseconds
    pub backspace_delay_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig {
                trigger_prefix: "//".to_string(),
                terminator: "alt".to_string(),
                timeout_seconds: 30,
                max_prompt_chars: 12_000,
                context_turns: 10,
                include_selected_text: false,
                stream_responses: false,
            },
            providers: ProvidersConfig {
                active_provider: Some("auto".to_string()),
                groq: Some(GroqConfig {
                    model: Some("openai/gpt-oss-120b".to_string()),
                }),
                gemini: Some(GeminiConfig {
                    default_model: Some("gemini-2.5-flash".to_string()),
                    legacy_api_key: None,
                }),
                openai_compatible: None,
            },
            routing: RoutingConfig {
                mode: "automatic".to_string(),
                strategy: "auto".to_string(),
                local_first: false,
                fallback: true,
            },
            injection: InjectionConfig {
                // Clipboard paste preserves punctuation, Unicode, and ordering
                // across Windows applications better than simulated typing.
                clipboard_threshold: 1,
                backspace_delay_ms: 0,
            },
        }
    }
}

impl Config {
    /// Returns the path to the KAI config directory.
    pub fn config_dir() -> PathBuf {
        if let Some(app_data) = dirs::config_dir() {
            app_data.join("kai")
        } else {
            PathBuf::from(".")
        }
    }

    /// Returns the path to the main config file.
    pub fn config_file() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Loads config from disk, or returns defaults if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let config_path = Self::config_file();

        if !config_path.exists() {
            info!("No config file found at {:?}", config_path);
            info!("Creating default config...");
            Self::create_default_config(&config_path)?;
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {:?}", config_path))?;

        let mut config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", config_path))?;

        let mut migrated_secret = false;
        if let Some(secret) = config
            .providers
            .gemini
            .as_mut()
            .and_then(|provider| provider.legacy_api_key.take())
            .filter(|secret| !secret.trim().is_empty())
        {
            crate::credentials::write_secret("KAI/GEMINI_API_KEY", secret.trim())
                .context("Could not migrate Gemini key to Windows Credential Manager")?;
            migrated_secret = true;
        }
        if let Some(secret) = config
            .providers
            .openai_compatible
            .as_mut()
            .and_then(|provider| provider.legacy_api_key.take())
            .filter(|secret| !secret.trim().is_empty())
        {
            crate::credentials::write_secret("KAI/OPENAI_COMPATIBLE_API_KEY", secret.trim())
                .context(
                    "Could not migrate compatible-provider key to Windows Credential Manager",
                )?;
            migrated_secret = true;
        }
        if migrated_secret {
            config.write_config()?;
            warn!("Moved legacy provider API keys from config.toml to Windows Credential Manager");
        }
        config.validate()?;

        info!("Config loaded from {:?}", config_path);
        Ok(config)
    }

    /// Validate and atomically persist the configuration.
    pub fn save(&self) -> Result<()> {
        self.validate()?;
        self.write_config()
    }

    fn write_config(&self) -> Result<()> {
        let path = Self::config_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
        }

        let content = toml::to_string_pretty(self).context("Failed to serialize configuration")?;
        let temp_path = path.with_extension("toml.tmp");
        fs::write(&temp_path, content)
            .with_context(|| format!("Failed to write temporary config: {:?}", temp_path))?;
        fs::rename(&temp_path, &path)
            .with_context(|| format!("Failed to replace configuration: {:?}", path))?;
        info!("Configuration saved to {:?}", path);
        Ok(())
    }

    /// Validates settings that could otherwise cause confusing runtime
    /// behavior or unsafe/inoperable input handling.
    pub fn validate(&self) -> Result<()> {
        if self.general.trigger_prefix.is_empty() {
            bail!("general.trigger_prefix must not be empty");
        }
        if self.general.timeout_seconds == 0 || self.general.timeout_seconds > 600 {
            bail!("general.timeout_seconds must be between 1 and 600");
        }
        if self.general.max_prompt_chars < 100 || self.general.max_prompt_chars > 1_000_000 {
            bail!("general.max_prompt_chars must be between 100 and 1,000,000");
        }
        if self.general.context_turns > 100 {
            bail!("general.context_turns must be 100 or less");
        }
        for model in [
            self.providers
                .groq
                .as_ref()
                .and_then(|provider| provider.model.as_ref()),
            self.providers
                .gemini
                .as_ref()
                .and_then(|provider| provider.default_model.as_ref()),
            self.providers
                .openai_compatible
                .as_ref()
                .map(|provider| &provider.default_model),
        ]
        .into_iter()
        .flatten()
        {
            if model.trim().is_empty() || model.len() > 200 {
                bail!("provider model IDs must contain 1 to 200 characters");
            }
        }
        if let Some(provider) = &self.providers.openai_compatible {
            if provider.base_url.len() > 2048 {
                bail!("providers.openai_compatible.base_url is too long");
            }
            let url = reqwest::Url::parse(&provider.base_url)
                .context("providers.openai_compatible.base_url must be an absolute URL")?;
            if !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                bail!("compatible-provider base URLs must not contain credentials, queries, or fragments");
            }
            let is_loopback = url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|address| address.is_loopback())
            });
            if !matches!(url.scheme(), "https") && !(url.scheme() == "http" && is_loopback) {
                bail!(
                    "compatible-provider URLs must use HTTPS, except for localhost HTTP endpoints"
                );
            }
        }
        if !matches!(
            self.routing.mode.to_ascii_lowercase().as_str(),
            "automatic" | "manual" | "trigger-based"
        ) {
            bail!("routing.mode must be automatic, manual, or trigger-based");
        }
        if !matches!(
            self.providers
                .active_provider
                .as_deref()
                .unwrap_or("auto")
                .to_ascii_lowercase()
                .as_str(),
            "auto" | "cloud" | "local" | "groq" | "gemini" | "openai-compatible"
        ) {
            bail!("providers.active_provider is not a supported provider route");
        }
        if self.routing.mode.eq_ignore_ascii_case("manual")
            && matches!(
                self.providers
                    .active_provider
                    .as_deref()
                    .unwrap_or("auto")
                    .to_ascii_lowercase()
                    .as_str(),
                "auto" | "cloud"
            )
        {
            bail!("manual routing requires a specific provider selection");
        }
        if !matches!(
            self.routing.strategy.to_ascii_lowercase().as_str(),
            "auto" | "cloud" | "local"
        ) {
            bail!("routing.strategy must be auto, cloud, or local");
        }
        if self.general.terminator.to_lowercase() != "alt" {
            bail!("general.terminator must currently be 'alt'");
        }
        if self.injection.clipboard_threshold == 0 {
            bail!("injection.clipboard_threshold must be greater than 0");
        }
        if self.injection.backspace_delay_ms > 1000 {
            bail!("injection.backspace_delay_ms must be 1000 or less");
        }
        Ok(())
    }

    /// Creates a default config file with comments explaining each option.
    fn create_default_config(path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
        }

        let default_content = r#"# KAI Configuration
# Universal AI Input Layer
# ========================

[general]
# The prefix that activates KAI. Type this prefix anywhere to start a trigger.
trigger_prefix = "//"

# How to terminate a prompt. Press ALT key to send.
terminator = "alt"

# Maximum seconds to wait for an AI response.
timeout_seconds = 30

# Maximum prompt characters accepted before dispatch.
max_prompt_chars = 12000

# Recent user/assistant turns retained in memory.
context_turns = 10

# Copy selected text into the AI request. Disabled by default for privacy.
include_selected_text = false

# Request streamed output when supported. KAI injects only the final response.
stream_responses = false

[providers.groq]
# Optional Groq model. KAI_MODEL and GROQ_MODEL override this value.
model = "openai/gpt-oss-120b"

[providers.gemini]
# Store API keys with --setup-credentials; keys do not belong in this file.
default_model = "gemini-2.5-flash"

[routing]
# Choose: "automatic", "manual", or "trigger-based".
mode = "automatic"
# Strategy: "auto", "cloud", or "local".
strategy = "auto"
# Prefer [providers.openai_compatible] in auto mode.
local_first = false
# Try another configured route after a provider failure.
fallback = true

[injection]
# Responses at or above this many characters use clipboard paste.
# Keep this at 1 to preserve exact text across Windows applications.
clipboard_threshold = 1

# Optional delay between backspace keystrokes (milliseconds). Set above 0 only
# if a target application drops rapid synthetic input.
backspace_delay_ms = 0
"#;

        fs::write(path, default_content)
            .with_context(|| format!("Failed to write default config: {:?}", path))?;

        info!("Default config created at {:?}", path);
        warn!(
            "Edit {:?} to set your API key, or set GROQ_API_KEY / GEMINI_API_KEY env variable.",
            path
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = Config::default();
        assert_eq!(config.injection.backspace_delay_ms, 0);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn empty_prefix_is_rejected() {
        let mut config = Config::default();
        config.general.trigger_prefix.clear();
        assert!(config.validate().is_err());
    }

    #[test]
    fn unsupported_terminator_is_rejected() {
        let mut config = Config::default();
        config.general.terminator = "enter".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn default_config_serializes() {
        let text = toml::to_string(&Config::default()).expect("default config should serialize");
        assert!(text.contains("trigger_prefix"));
        assert!(text.contains("active_provider"));
        assert!(text.contains("[providers.groq]"));
        assert!(text.contains("openai/gpt-oss-120b"));
    }
}
