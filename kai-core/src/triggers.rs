// ============================================================
// triggers.rs — YAML-driven trigger configuration
//
// Loads %APPDATA%\kai\triggers.yml on startup.
// Auto-creates the file with all defaults on first run.
//
// Each trigger entry in the YAML has:
//   name:       the command word (e.g. "ask" -> type "//ask ...")
//   aliases:    optional list of alternative names
//   prompt:     the AI system instruction for this trigger
//   max_tokens: optional max response length (default: 2000)
//
// Usage:
//   let triggers = TriggersFile::load()?;
//   let (system_prompt, max_tokens, provider, model) = triggers.resolve_route("ask");
// ============================================================

use anyhow::bail;
use anyhow::{Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::config::Config;

const MAX_TRIGGER_INSTRUCTION_CHARS: usize = 20_000;

/// A single trigger definition from triggers.yml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDef {
    /// Primary command name (e.g. "ask", "coder", "fix")
    pub name: String,
    /// Alternative names that map to the same trigger (e.g. ["code", "dev"])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// The AI system prompt for this trigger
    pub prompt: String,
    /// Max tokens for the response (optional, defaults to 2000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Optional provider override: groq, gemini, or openai-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Optional provider model identifier for this trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// The top-level structure of triggers.yml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggersFile {
    /// All defined triggers
    pub triggers: Vec<TriggerDef>,
    /// Fallback prompt used when no trigger matches the command
    pub fallback_prompt: String,
}

impl TriggersFile {
    /// Returns the path to the triggers YAML file.
    pub fn triggers_file() -> PathBuf {
        Config::config_dir().join("triggers.yml")
    }

    /// Loads triggers from disk, or creates the default file if it doesn't exist.
    pub fn load() -> Result<Self> {
        let path = Self::triggers_file();

        if !path.exists() {
            info!("No triggers.yml found at {:?}", path);
            info!("Creating default triggers.yml...");
            Self::create_default(&path)?;
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read triggers.yml: {:?}", path))?;

        let triggers: TriggersFile = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse triggers.yml: {:?}", path))?;

        triggers.validate()?;

        info!(
            "Loaded {} trigger(s) from {:?}",
            triggers.triggers.len(),
            path
        );
        for t in &triggers.triggers {
            let alias_str = if t.aliases.is_empty() {
                String::new()
            } else {
                format!(" (aliases: {})", t.aliases.join(", "))
            };
            info!("  //{}{}", t.name, alias_str);
        }

        Ok(triggers)
    }

    /// Resolves prompt settings and an optional provider/model route.
    pub fn resolve_route(&self, command: &str) -> (&str, u32, Option<&str>, Option<&str>) {
        let cmd = command.to_lowercase();
        for trigger in &self.triggers {
            if trigger.name == cmd || trigger.aliases.iter().any(|a| a == &cmd) {
                return (
                    &trigger.prompt,
                    trigger.max_tokens.unwrap_or(2000),
                    trigger.provider.as_deref(),
                    trigger.model.as_deref(),
                );
            }
        }
        warn!(
            "No trigger found for command '{}' — using fallback",
            command
        );
        (&self.fallback_prompt, 2000, None, None)
    }

    /// Returns whether a command or alias is explicitly configured.
    pub fn contains(&self, command: &str) -> bool {
        let cmd = command.to_lowercase();
        if cmd == "help" || cmd == "list" {
            return true;
        }
        self.triggers
            .iter()
            .any(|trigger| trigger.name == cmd || trigger.aliases.iter().any(|alias| alias == &cmd))
    }

    /// Validates trigger names and aliases before they can be used.
    pub fn validate(&self) -> Result<()> {
        if self.fallback_prompt.trim().is_empty() {
            bail!("fallback_prompt must not be empty");
        }
        if self.fallback_prompt.chars().count() > MAX_TRIGGER_INSTRUCTION_CHARS {
            bail!(
                "fallback_prompt exceeds the {} character limit",
                MAX_TRIGGER_INSTRUCTION_CHARS
            );
        }
        let mut names = std::collections::HashSet::new();
        for trigger in &self.triggers {
            if trigger.name.trim().is_empty() || trigger.prompt.trim().is_empty() {
                bail!("trigger names and prompts must not be empty");
            }
            if trigger.prompt.chars().count() > MAX_TRIGGER_INSTRUCTION_CHARS {
                bail!(
                    "prompt for trigger '{}' exceeds the {} character limit",
                    trigger.name,
                    MAX_TRIGGER_INSTRUCTION_CHARS
                );
            }
            if let Some(max_tokens) = trigger.max_tokens {
                if !(1..=128_000).contains(&max_tokens) {
                    bail!(
                        "max_tokens for trigger '{}' must be between 1 and 128000",
                        trigger.name
                    );
                }
            }
            if let Some(provider) = &trigger.provider {
                if !matches!(
                    provider.to_ascii_lowercase().as_str(),
                    "groq" | "gemini" | "openai-compatible"
                ) {
                    bail!(
                        "unsupported provider on trigger '{}': {}",
                        trigger.name,
                        provider
                    );
                }
            }
            if trigger
                .model
                .as_ref()
                .is_some_and(|model| model.trim().is_empty() || model.len() > 200)
            {
                bail!(
                    "model on trigger '{}' must contain 1 to 200 characters",
                    trigger.name
                );
            }
            if trigger.model.is_some() && trigger.provider.is_none() {
                bail!(
                    "trigger '{}' must set a provider when it sets a model",
                    trigger.name
                );
            }
            for name in std::iter::once(&trigger.name).chain(trigger.aliases.iter()) {
                let normalized = name.to_lowercase();
                if name.len() > 64
                    || !name.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                    })
                {
                    bail!("trigger names and aliases may use only letters, digits, '-' and '_' (max 64 characters)");
                }
                if normalized == "help" || normalized == "list" || !names.insert(normalized.clone())
                {
                    bail!("duplicate or reserved trigger name: {}", name);
                }
            }
        }
        Ok(())
    }

    /// Validates and atomically saves trigger definitions as YAML.
    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let path = Self::triggers_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir: {:?}", parent))?;
        }
        let yaml =
            serde_yaml::to_string(self).context("Failed to serialize trigger configuration")?;
        let content = format!(
            "# KAI Triggers Configuration\n# Edit through KAI Settings or this YAML file.\n# Provider/model overrides require routing.mode = \"trigger-based\".\n\n{yaml}"
        );
        let temp_path = path.with_extension("yml.tmp");
        fs::write(&temp_path, content)
            .with_context(|| format!("Failed to write temporary trigger file: {:?}", temp_path))?;
        fs::rename(&temp_path, &path)
            .with_context(|| format!("Failed to replace trigger file: {:?}", path))?;
        Ok(())
    }

    /// Returns the locally available command names and aliases.
    pub fn help_text(&self, prefix: &str) -> String {
        let mut output = format!(
            "KAI commands:\n  {}help or {}list — show this list\n",
            prefix, prefix
        );
        for trigger in &self.triggers {
            output.push_str("  ");
            output.push_str(prefix);
            output.push_str(&trigger.name);
            if !trigger.aliases.is_empty() {
                output.push_str(" (aliases: ");
                output.push_str(&trigger.aliases.join(", "));
                output.push(')');
            }
            output.push('\n');
        }
        output
    }

    /// Creates the default triggers.yml with all built-in triggers.
    fn create_default(path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir: {:?}", parent))?;
        }

        let default_content = r#"# KAI Triggers Configuration
# ===========================
# Add, edit, or remove triggers here. Restart KAI to apply changes.
#
# How to use:
#   In any text field, type the prefix followed by a command and your prompt.
#   Example: "//ask what is the capital of France"
#   Then press ALT to send.
#
# Trigger format:
#   name:       The command word (e.g. "ask" -> type "//ask ...")
#   aliases:    Other names that do the same thing (optional)
#   prompt:     The AI system instruction for this trigger
#   max_tokens: Max response length (optional, default: 2000)
#   provider:   Optional override: groq, gemini, or openai-compatible
#   model:      Optional model ID supported by that provider
#   Note:       Provider/model are used only when routing.mode = "trigger-based".
#
# To ADD a new trigger:  add a new block under "triggers:"
# To EDIT a trigger:     change its "prompt:" field
# To REMOVE a trigger:   delete its block
# To ADD an alias:       add it to "aliases:" list

triggers:

  - name: human
    aliases: [me]
    max_tokens: 2000
    # provider: groq
    # model: openai/gpt-oss-120b
    prompt: >
        You are a human. Respond to the user as a human would, with empathy and understanding.
        Use real world knowledge and common sense. Avoid AI jargon or disclaimers.
      

  - name: coder
    aliases: [code, dev]
    max_tokens: 3000
    prompt: >
      You are an expert programming assistant and software engineer.
      Provide clear, accurate answers with code examples when relevant.
      Keep code blocks minimal, no markdown fences, just indented code.
      Focus on correctness and clarity.

  - name: fix
    max_tokens: 1000
    prompt: >
      You are a professional writing editor.
      Improve the given text for clarity, grammar, tone, and professionalism.
      Return ONLY the improved text, no explanations, no prefixes, no commentary.
      Preserve the original meaning and voice.

  - name: explain
    aliases: [eli5]
    max_tokens: 500
    prompt: >
      Explain the following concept simply and clearly, as if to a smart person
      who is unfamiliar with the topic. Use analogies where helpful.
      Keep it concise, under 150 words unless complexity demands more.
      Plain text only, no markdown.

  - name: translate
    max_tokens: 1000
    prompt: >
      Translate the following text. If it is in English, translate to Spanish.
      If it is in another language, translate to English.
      Return ONLY the translation, no explanations, no labels.

  - name: summarize
    aliases: [summary, tldr]
    max_tokens: 500
    prompt: >
      Summarize the following content in 2-3 concise sentences.
      Capture the key points. Return ONLY the summary, no preamble.

  - name: email
    max_tokens: 1000
    prompt: >
      You are a professional business communication expert.
      Write or rewrite the following as a polished, professional email.
      Keep it concise and clear. Plain text only.

  - name: tweet
    aliases: [post]
    max_tokens: 300
    prompt: >
      Rewrite the following as an engaging social media post.
      Be concise, punchy, and human. Plain text only, no hashtag spam.

# Fallback prompt used when no trigger name matches
fallback_prompt: >
  You are a helpful AI assistant.
  Answer the user's request clearly and concisely.
  Plain text only, no markdown formatting.
"#;

        fs::write(path, default_content)
            .with_context(|| format!("Failed to write default triggers.yml: {:?}", path))?;

        info!("Default triggers.yml created at {:?}", path);
        warn!(
            "Edit {:?} to customize your triggers. Restart KAI to apply changes.",
            path
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definitions() -> TriggersFile {
        TriggersFile {
            triggers: vec![TriggerDef {
                name: "coder".to_string(),
                aliases: vec!["code".to_string()],
                prompt: "coding assistant".to_string(),
                max_tokens: Some(1000),
                provider: Some("groq".to_string()),
                model: Some("openai/gpt-oss-120b".to_string()),
            }],
            fallback_prompt: "fallback".to_string(),
        }
    }

    #[test]
    fn contains_known_command_and_alias() {
        let triggers = definitions();
        assert!(triggers.contains("coder"));
        assert!(triggers.contains("CODE"));
        assert!(!triggers.contains("unknown"));
    }

    #[test]
    fn resolve_route_returns_all_configured_settings() {
        let triggers = definitions();
        let (prompt, max_tokens, provider, model) = triggers.resolve_route("code");
        assert_eq!(prompt, "coding assistant");
        assert_eq!(max_tokens, 1000);
        assert_eq!(provider, Some("groq"));
        assert_eq!(model, Some("openai/gpt-oss-120b"));
    }

    #[test]
    fn validation_rejects_duplicate_names() {
        let mut triggers = definitions();
        triggers.triggers.push(TriggerDef {
            name: "CODE".to_string(),
            aliases: vec![],
            prompt: "duplicate".to_string(),
            max_tokens: None,
            provider: None,
            model: None,
        });
        assert!(triggers.validate().is_err());
    }

    #[test]
    fn help_lists_local_commands() {
        let text = definitions().help_text("//");
        assert!(text.contains("//help"));
        assert!(text.contains("//coder (aliases: code)"));
    }
}
