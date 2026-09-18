// ============================================================
// KAI Core — main.rs
// Entry point: Main thread runs the Win32 global keyboard hook &
// message loop, while Tokio background threads handle trigger parsing
// and AI completions.
// ============================================================

mod config;
mod injector;
mod listener;
mod parser;
mod providers;
mod state;
mod trigger;
mod triggers;

use anyhow::Result;
use log::{error, info, warn};
use std::sync::Arc;
use tokio::sync::mpsc;

use config::Config;
use listener::KeyEvent;
use providers::openai::OpenAiProvider;
use providers::AiProvider;
use state::AppState;
use trigger::TriggerEngine;
use triggers::TriggersFile;

fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("╔══════════════════════════════════════╗");
    info!("║   KAI — Universal AI Input Layer     ║");
    info!("║   Version 0.1.0 (Phase 1 MVP)        ║");
    info!("╚══════════════════════════════════════╝");

    // Load configuration
    let config = Config::load().unwrap_or_else(|e| {
        eprintln!("Failed to load config: {}", e);
        Config::default()
    });

    // Load YAML trigger definitions
    let trigger_defs = Arc::new(TriggersFile::load().unwrap_or_else(|e| {
        eprintln!("Failed to load triggers.yml: {}", e);
        eprintln!("Using built-in fallback triggers.");
        TriggersFile {
            triggers: vec![],
            fallback_prompt: "You are a helpful AI assistant. Answer clearly and concisely. Plain text only.".to_string(),
        }
    }));

    // Create Tokio async runtime for background tasks (AI API & trigger processing)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // Determine AI provider & API key (Groq or Gemini)
    let groq_key = std::env::var("GROQ_API_KEY").ok();

    let gemini_key = std::env::var("GEMINI_API_KEY").ok()
        .or_else(|| config.providers.gemini.as_ref().and_then(|p| p.api_key.clone()));

    let provider: Arc<dyn AiProvider> = if let Some(key) = groq_key {
        let model = std::env::var("GROQ_MODEL")
            .ok()
            .unwrap_or_else(|| "openai/gpt-oss-120b".to_string());
        let base_url = "https://api.groq.com/openai/v1".to_string();
        info!("Using Groq Cloud Provider (model: {}, 14,400 free requests/day) ✓", model);
        Arc::new(OpenAiProvider::with_base_url(key, model, base_url))
    } else if let Some(key) = gemini_key {
        let model = config.providers.gemini.as_ref()
            .and_then(|p| p.default_model.clone())
            .unwrap_or_else(|| "gemini-2.5-flash".to_string());
        info!("Using Google Gemini Provider (model: {}) ✓", model);
        Arc::new(providers::gemini::GeminiProvider::new(key, model))
    } else {
        error!("No AI API key found!");
        error!("Set GROQ_API_KEY or GEMINI_API_KEY environment variable, or edit config:");
        error!("  %APPDATA%\\kai\\config.toml");
        std::process::exit(1);
    };

    info!("Trigger prefix: \"{}\"", config.general.trigger_prefix);
    info!("Listening for triggers...");
    info!("");
    info!("Try typing  //ask <your question>  in any application, then press ALT.");
    info!("Press Ctrl+C to exit.");

    // Shared application state
    let app_state = Arc::new(AppState::new());

    // Channels
    let (key_tx, key_rx) = mpsc::unbounded_channel::<KeyEvent>();
    let (trigger_tx, trigger_rx) = mpsc::unbounded_channel::<trigger::CapturedTrigger>();

    // Spawn async tasks on background Tokio runtime
    let config_for_trigger = config.clone();
    rt.spawn(async move {
        let mut engine = TriggerEngine::new(config_for_trigger, trigger_tx);
        engine.run(key_rx).await;
    });

    let state_for_ai = Arc::clone(&app_state);
    let provider_for_ai = Arc::clone(&provider);
    let trigger_defs_for_ai = Arc::clone(&trigger_defs);
    rt.spawn(async move {
        run_ai_dispatcher(trigger_rx, provider_for_ai, trigger_defs_for_ai, state_for_ai).await;
    });

    // CRITICAL FOR WIN32: The main thread MUST install the hook and run the GetMessage pump
    listener::start_listener(key_tx);

    Ok(())
}

/// Receives captured triggers, calls the AI provider, and injects the response.
async fn run_ai_dispatcher(
    mut rx: mpsc::UnboundedReceiver<trigger::CapturedTrigger>,
    provider: Arc<dyn AiProvider>,
    trigger_defs: Arc<TriggersFile>,
    state: Arc<AppState>,
) {
    use injector::TextInjector;
    use parser::parse_trigger;

    let mut injector = TextInjector::new();

    while let Some(captured) = rx.recv().await {
        info!("▶ Trigger captured: {:?}", captured.raw_text);

        // Parse the trigger into command + prompt
        let parsed = match parse_trigger(&captured.raw_text) {
            Some(p) => p,
            None => {
                warn!("Could not parse trigger: {}", captured.raw_text);
                continue;
            }
        };

        info!("  Command : {}", parsed.command);
        info!("  Prompt  : {}", parsed.prompt);

        // Mark as processing
        state.set_processing(true);

        // Build system prompt from YAML trigger definitions
        let (system_prompt, max_tokens) = trigger_defs.resolve(&parsed.command);

        // Call AI provider
        let request = providers::CompletionRequest {
            system_prompt: system_prompt.to_string(),
            user_prompt: parsed.prompt.clone(),
            max_tokens: Some(max_tokens),
        };

        match provider.complete(request).await {
            Ok(response) => {
                info!("  Response: {} chars", response.text.len());

                // First, erase the original trigger text
                injector.delete_chars(captured.char_count);

                // Small delay to let the deletion settle
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                // Inject the AI response
                if let Err(e) = injector.inject_text(&response.text) {
                    error!("Failed to inject text: {}", e);
                } else {
                    info!("  ✓ Response injected successfully");
                }
            }
            Err(e) => {
                error!("AI provider error: {}", e);
                injector.delete_chars(captured.char_count);
                let _ = injector.inject_text(&format!("[KAI Error: {}]", e));
            }
        }

        state.set_processing(false);
    }
}

