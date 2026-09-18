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
//   let (system_prompt, max_tokens) = triggers.resolve("ask");
// ============================================================

use anyhow::{Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::config::Config;

/// A single trigger definition from triggers.yml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDef {
    /// Primary command name (e.g. "ask", "coder", "fix")
    pub name: String,
    /// Alternative names that map to the same trigger (e.g. ["code", "dev"])
    #[serde(default)]
    pub aliases: Vec<String>,
    /// The AI system prompt for this trigger
    pub prompt: String,
    /// Max tokens for the response (optional, defaults to 2000)
    pub max_tokens: Option<u32>,
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

        info!("Loaded {} trigger(s) from {:?}", triggers.triggers.len(), path);
        for t in &triggers.triggers {
            let alias_str = if t.aliases.is_empty() {
                String::new()
            } else {
                format!(" (aliases: {})", t.aliases.join(", "))
            };
            info!("  //{}{}",  t.name, alias_str);
        }

        Ok(triggers)
    }

    /// Resolves a command name (or alias) to its (system_prompt, max_tokens).
    /// Falls back to (fallback_prompt, 2000) if nothing matches.
    pub fn resolve(&self, command: &str) -> (&str, u32) {
        let cmd = command.to_lowercase();
        for trigger in &self.triggers {
            if trigger.name == cmd || trigger.aliases.iter().any(|a| a == &cmd) {
                return (&trigger.prompt, trigger.max_tokens.unwrap_or(2000));
            }
        }
        warn!("No trigger found for command '{}' — using fallback", command);
        (&self.fallback_prompt, 2000)
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
#
# To ADD a new trigger:  add a new block under "triggers:"
# To EDIT a trigger:     change its "prompt:" field
# To REMOVE a trigger:   delete its block
# To ADD an alias:       add it to "aliases:" list

triggers:

  - name: human
    aliases: [me]
    max_tokens: 2000
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
        warn!("Edit {:?} to customize your triggers. Restart KAI to apply changes.", path);

        Ok(())
    }
}
