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
    pub injection: InjectionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// The prefix that triggers KAI (default: ";;")
    pub trigger_prefix: String,
    /// How to terminate a prompt — "enter" is the only mode in Phase 1
    pub terminator: String,
    /// Maximum seconds to wait for an AI response before giving up
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    pub active_provider: Option<String>,
    pub gemini: Option<GeminiConfig>,
}



#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    pub api_key: Option<String>,
    pub default_model: Option<String>,
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
            },
            providers: ProvidersConfig {
                active_provider: Some("auto".to_string()),
                gemini: Some(GeminiConfig {
                    api_key: None,
                    default_model: Some("gemini-2.5-flash".to_string()),
                }),
            },
            injection: InjectionConfig {
                clipboard_threshold: 200,
                backspace_delay_ms: 5,
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

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", config_path))?;

        info!("Config loaded from {:?}", config_path);
        Ok(config)
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

[providers.gemini]
# api_key = "your-gemini-key-here"
default_model = "gemini-2.5-flash"

[injection]
# Text longer than this many characters uses clipboard paste (faster).
# Text shorter than this uses simulated keystrokes (safer for all apps).
clipboard_threshold = 200

# Delay between backspace keystrokes when erasing trigger text (milliseconds).
backspace_delay_ms = 5
"#;

        fs::write(path, default_content)
            .with_context(|| format!("Failed to write default config: {:?}", path))?;

        info!("Default config created at {:?}", path);
        warn!("Edit {:?} to set your API key, or set GROQ_API_KEY / GEMINI_API_KEY env variable.", path);

        Ok(())
    }
}
