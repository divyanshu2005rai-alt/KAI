// ============================================================
// KAI Core — main.rs
// Entry point: Main thread runs the Win32 global keyboard hook &
// message loop, while Tokio background threads handle trigger parsing
// and AI completions.
// ============================================================

mod config;
mod context;
mod credentials;
mod injector;
mod listener;
mod parser;
mod providers;
mod settings;
mod state;
mod tray;
mod trigger;
mod triggers;

use anyhow::Result;
use log::{error, info, warn};
use std::sync::Arc;
use tokio::sync::mpsc;

use config::Config;
use listener::KeyEvent;
use providers::AiProvider;
use state::AppState;
use trigger::TriggerEngine;
use triggers::TriggersFile;
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--setup-credentials") {
        setup_credentials()?;
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--configure") {
        settings::SettingsService::interactive()?;
        return Ok(());
    }

    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("╔══════════════════════════════════════╗");
    info!("║   KAI — Universal AI Input Layer     ║");
    info!("║   Version 0.1.0                       ║");
    info!("╚══════════════════════════════════════╝");

    // Load configuration
    let config = settings::SettingsService::load().unwrap_or_else(|e| {
        eprintln!("Failed to load config: {}", e);
        Config::default()
    });

    // Load YAML trigger definitions
    let trigger_defs = Arc::new(TriggersFile::load().unwrap_or_else(|e| {
        eprintln!("Failed to load triggers.yml: {}", e);
        eprintln!("Using built-in fallback triggers.");
        TriggersFile {
            triggers: vec![],
            fallback_prompt:
                "You are a helpful AI assistant. Answer clearly and concisely. Plain text only."
                    .to_string(),
        }
    }));

    // Create Tokio async runtime for background tasks (AI API & trigger processing)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // Provider construction is isolated behind one factory so future Qwen,
    // local, and vision providers do not require changes to the app loop.
    let provider: Arc<dyn AiProvider> = providers::factory::build(&config).unwrap_or_else(|e| {
        error!("No usable AI provider: {}", e);
        error!("Set GROQ_API_KEY or GEMINI_API_KEY, or configure a provider in %APPDATA%\\kai\\config.toml");
        std::process::exit(1);
    });

    info!("Trigger prefix: \"{}\"", config.general.trigger_prefix);
    info!("Listening for triggers...");
    info!("");
    info!("Try typing  //ask <your question>  in any application, then press ALT.");
    info!("Press Ctrl+C to exit.");

    // Shared application state
    let app_state = Arc::new(AppState::with_history_limit(config.general.context_turns));
    if let Err(error) = tray::start(Arc::clone(&app_state)) {
        warn!(
            "System tray unavailable; terminal controls remain active: {}",
            error
        );
    }

    // Channels
    let (key_tx, key_rx) = mpsc::unbounded_channel::<KeyEvent>();
    let (trigger_tx, trigger_rx) = mpsc::unbounded_channel::<trigger::CapturedTrigger>();

    // Spawn async tasks on background Tokio runtime
    let config_for_trigger = config.clone();
    let state_for_trigger = Arc::clone(&app_state);
    rt.spawn(async move {
        let mut engine = TriggerEngine::new(config_for_trigger, trigger_tx, state_for_trigger);
        engine.run(key_rx).await;
    });

    // Temporary control surface shared by the future system tray UI.
    // Commands: p = pause/resume, s = status, q = quit.
    let state_for_controls = Arc::clone(&app_state);
    std::thread::spawn(move || {
        use std::io::{self, BufRead};
        for line in io::stdin().lock().lines().flatten() {
            match line.trim().to_ascii_lowercase().as_str() {
                "p" | "pause" | "resume" => {
                    let paused = state_for_controls.toggle_paused();
                    info!("KAI {}", if paused { "paused" } else { "resumed" });
                }
                "s" | "status" => {
                    info!(
                        "KAI status: {}, processing: {}",
                        if state_for_controls.is_paused() {
                            "paused"
                        } else {
                            "active"
                        },
                        state_for_controls.is_processing()
                    );
                }
                "q" | "quit" | "exit" => {
                    info!("KAI shutting down from control command");
                    std::process::exit(0);
                }
                _ => info!("Control commands: p=pause/resume, s=status, q=quit"),
            }
        }
    });

    let state_for_ai = Arc::clone(&app_state);
    let provider_for_ai = Arc::clone(&provider);
    let trigger_defs_for_ai = Arc::clone(&trigger_defs);
    let config_for_ai = config.clone();
    rt.spawn(async move {
        run_ai_dispatcher(
            trigger_rx,
            provider_for_ai,
            trigger_defs_for_ai,
            state_for_ai,
            config_for_ai,
        )
        .await;
    });

    // CRITICAL FOR WIN32: The main thread MUST install the hook and run the GetMessage pump
    listener::start_listener(key_tx);

    Ok(())
}

fn setup_credentials() -> Result<()> {
    println!("KAI secure credential setup");
    println!("Leave a value empty to skip it.");

    let groq = rpassword::prompt_password("Groq API key: ")?;
    if !groq.trim().is_empty() {
        credentials::write_secret("KAI/GROQ_API_KEY", groq.trim())?;
        println!("Groq credential saved to Windows Credential Manager.");
    }

    let gemini = rpassword::prompt_password("Gemini API key: ")?;
    if !gemini.trim().is_empty() {
        credentials::write_secret("KAI/GEMINI_API_KEY", gemini.trim())?;
        println!("Gemini credential saved to Windows Credential Manager.");
    }
    Ok(())
}

/// Receives captured triggers, calls the AI provider, and injects the response.
async fn run_ai_dispatcher(
    mut rx: mpsc::UnboundedReceiver<trigger::CapturedTrigger>,
    provider: Arc<dyn AiProvider>,
    trigger_defs: Arc<TriggersFile>,
    state: Arc<AppState>,
    config: Config,
) {
    use injector::TextInjector;
    use parser::parse_trigger;

    let mut injector = TextInjector::new(
        config.injection.backspace_delay_ms,
        config.injection.clipboard_threshold,
    );

    while let Some(captured) = rx.recv().await {
        info!("▶ Trigger captured");

        // Parse the trigger into command + prompt
        let parsed = match parse_trigger(&captured.raw_text) {
            Some(p) => p,
            None => {
                warn!("Could not parse trigger");
                continue;
            }
        };

        info!("  Command : {}", parsed.command);
        info!(
            "  Prompt captured ({} characters)",
            parsed.prompt.chars().count()
        );

        if parsed.command == "help" || parsed.command == "list" {
            injector.delete_chars(captured.char_count);
            let _ = injector.inject_text(&trigger_defs.help_text(&config.general.trigger_prefix));
            continue;
        }

        // Do not silently treat misspelled or unsupported commands as a
        // generic request. This makes trigger categories predictable and
        // gives the user an actionable response.
        if !trigger_defs.contains(&parsed.command) {
            let message = format!(
                "[KAI Error: unknown command '{}'. Edit triggers.yml to add it.]",
                parsed.command
            );
            warn!("Unknown trigger command: {}", parsed.command);
            injector.delete_chars(captured.char_count);
            let _ = injector.inject_text(&message);
            continue;
        }

        if parsed.prompt.chars().count() > config.general.max_prompt_chars {
            let message = format!(
                "[KAI Error: input is too large (maximum {} characters).]",
                config.general.max_prompt_chars
            );
            warn!(
                "Rejecting oversized prompt: {} characters",
                parsed.prompt.chars().count()
            );
            injector.delete_chars(captured.char_count);
            let _ = injector.inject_text(&message);
            continue;
        }

        // Mark as processing
        state.set_processing(true);

        // Build system prompt from YAML trigger definitions
        let (system_prompt, max_tokens) = trigger_defs.resolve(&parsed.command);

        // Call AI provider
        let source_window = foreground_window();
        let context_snapshot =
            context::capture(source_window, config.general.include_selected_text);
        let system_prompt = match context_snapshot.prompt_block() {
            Some(context_block) => format!("{system_prompt}\n\n{context_block}"),
            None => system_prompt.to_string(),
        };

        let request = providers::CompletionRequest {
            system_prompt,
            user_prompt: parsed.prompt.clone(),
            history: state.conversation_history(),
            max_tokens: Some(max_tokens),
            stream: config.general.stream_responses,
        };

        let use_streaming = config.general.stream_responses && provider.supports_streaming();
        if config.general.stream_responses && !use_streaming {
            info!(
                "Provider {} does not support streaming; using complete response mode",
                provider.name()
            );
        }
        let completion = tokio::time::timeout(
            tokio::time::Duration::from_secs(config.general.timeout_seconds),
            async {
                if use_streaming {
                    provider.complete_stream(request).await
                } else {
                    provider.complete(request).await
                }
            },
        )
        .await;

        match completion {
            Err(_) => {
                error!("AI provider request timed out");
                injector.delete_chars(captured.char_count);
                let _ = injector.inject_text(
                    "[KAI Error: request timed out. Check your connection and try again.]",
                );
            }
            Ok(Err(e)) => {
                error!("AI provider error: {}", e);
                injector.delete_chars(captured.char_count);
                let _ = injector.inject_text(&format!("[KAI Error: {}]", e));
            }
            Ok(Ok(response)) => {
                info!("  Response: {} chars", response.text.len());
                state.remember_turn(parsed.prompt.clone(), response.text.clone());

                if response.text.trim().is_empty() {
                    warn!("AI provider returned an empty response");
                    injector.delete_chars(captured.char_count);
                    let _ = injector.inject_text(
                        "[KAI Error: the provider returned an empty response. Please try again.]",
                    );
                    state.set_processing(false);
                    continue;
                }

                if is_repetitive_corruption(&response.text) {
                    warn!("Provider returned a suspiciously repetitive response");
                    injector.delete_chars(captured.char_count);
                    let _ = injector.inject_text(
                        "[KAI Error: the provider returned corrupted repetitive output. Please retry.]",
                    );
                    state.set_processing(false);
                    continue;
                }

                // Never delete or inject into a different application than
                // the one that was active when the request started.
                if foreground_window() != source_window {
                    warn!("Active window changed while processing; using clipboard fallback");
                    if let Err(e) = injector.copy_to_clipboard(&response.text) {
                        error!("Clipboard fallback failed after focus change: {}", e);
                    }
                    state.set_processing(false);
                    continue;
                }

                // First, erase the original trigger text
                injector.delete_chars(captured.char_count);

                // Small delay to let the deletion settle
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                // Inject the AI response
                if let Err(e) = injector.inject_text(&response.text) {
                    error!("Failed to inject text: {}", e);
                    if let Err(copy_error) = injector.copy_to_clipboard(&response.text) {
                        error!("Clipboard fallback also failed: {}", copy_error);
                    } else {
                        warn!("Response was copied to clipboard; paste it manually");
                    }
                } else {
                    info!("  ✓ Response injected successfully");
                }
            }
        }

        state.set_processing(false);
    }
}

fn foreground_window() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

fn is_repetitive_corruption(text: &str) -> bool {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 32 {
        return false;
    }
    let first = chars[0];
    let repeated = chars
        .iter()
        .filter(|&&character| character == first)
        .count();
    repeated * 100 / chars.len() >= 90
}

#[cfg(test)]
mod response_tests {
    use super::is_repetitive_corruption;

    #[test]
    fn rejects_long_repeated_output() {
        assert!(is_repetitive_corruption(&"x".repeat(100)));
    }

    #[test]
    fn allows_normal_text() {
        assert!(!is_repetitive_corruption(
            "This is a normal response with repeated words and formatting."
        ));
    }
}
