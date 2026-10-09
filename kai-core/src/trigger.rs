// ============================================================
// trigger.rs — Rolling buffer & trigger state machine
//
// Implements a privacy-first trigger detection engine:
//   - In Idle state: maintains a tiny rolling buffer (max ~10 chars)
//     only large enough to detect the "//" prefix.
//   - On trigger match: switches to Capturing mode, collecting the
//     full command + prompt.
//   - On Alt: fires the captured trigger downstream.
//   - On Escape: cancels and returns to Idle.
//   - Non-character keys (arrows, etc.) while Idle: reset buffer.
// ============================================================

use log::{debug, info};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::config::Config;
use crate::listener::KeyEvent;
use crate::state::AppState;

/// A fully captured trigger, ready for AI dispatch.
#[derive(Debug, Clone)]
pub struct CapturedTrigger {
    /// The raw text as the user typed it (e.g. "//ask explain recursion")
    pub raw_text: String,
    /// Total number of visible characters typed (for backspace deletion)
    pub char_count: usize,
}

/// Internal state machine states.
#[derive(Debug, PartialEq)]
enum State {
    /// Normal typing — watching for trigger prefix in rolling buffer.
    Idle,
    /// Trigger prefix detected — capturing the command + prompt.
    Capturing,
}

/// The trigger detection engine.
pub struct TriggerEngine {
    config: Config,
    tx: UnboundedSender<CapturedTrigger>,
    state: State,
    /// Rolling buffer for Idle state — only last N chars
    idle_buffer: VecDeque<char>,
    /// Full capture buffer — the complete text after "//"
    capture_buffer: String,
    /// Total characters typed since trigger start (including "//" prefix)
    total_chars: usize,
    app_state: Arc<AppState>,
}

impl TriggerEngine {
    pub fn new(
        config: Config,
        tx: UnboundedSender<CapturedTrigger>,
        app_state: Arc<AppState>,
    ) -> Self {
        let prefix_len = config.general.trigger_prefix.len() + 20; // buffer headroom
        Self {
            config,
            tx,
            state: State::Idle,
            idle_buffer: VecDeque::with_capacity(prefix_len),
            capture_buffer: String::new(),
            total_chars: 0,
            app_state,
        }
    }

    /// Main processing loop — receives key events and drives the state machine.
    pub async fn run(&mut self, mut rx: UnboundedReceiver<KeyEvent>) {
        while let Some(event) = rx.recv().await {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: KeyEvent) {
        if self.app_state.is_paused() {
            self.reset();
            return;
        }
        match &self.state {
            State::Idle => self.handle_idle(event),
            State::Capturing => self.handle_capturing(event),
        }
    }

    // ── Idle state handlers ─────────────────────────────────────

    fn handle_idle(&mut self, event: KeyEvent) {
        match event {
            KeyEvent::Char(c) => {
                // Add to rolling buffer
                self.idle_buffer.push_back(c);

                // Keep buffer trimmed to prefix length + small headroom
                let max_len = self.config.general.trigger_prefix.len() + 2;
                while self.idle_buffer.len() > max_len {
                    self.idle_buffer.pop_front();
                }

                // Check if buffer ends with our trigger prefix (e.g. "//")
                let buffer_str: String = self.idle_buffer.iter().collect();
                if buffer_str.ends_with(&self.config.general.trigger_prefix) {
                    info!("⚡ Trigger prefix detected — entering capture mode");
                    self.enter_capture_mode();
                }
            }
            KeyEvent::Enter | KeyEvent::Escape | KeyEvent::Backspace | KeyEvent::Alt => {
                self.idle_buffer.clear();
            }
            KeyEvent::Other => {
                // Do not clear idle buffer on modifier keys (Shift, Ctrl, Alt)
            }
        }
    }

    fn enter_capture_mode(&mut self) {
        self.state = State::Capturing;
        self.capture_buffer.clear();
        // Count the prefix characters ("//" = 2 chars)
        self.total_chars = self.config.general.trigger_prefix.len();
        self.idle_buffer.clear();
        debug!(
            "Capture mode active. total_chars starts at {}",
            self.total_chars
        );
    }

    // ── Capturing state handlers ────────────────────────────────

    fn handle_capturing(&mut self, event: KeyEvent) {
        match event {
            KeyEvent::Char(c) => {
                self.capture_buffer.push(c);
                self.total_chars += 1;
                debug!(
                    "Captured input character (total chars: {})",
                    self.total_chars
                );
            }

            KeyEvent::Enter => {
                // Enter key while capturing allows multi-line prompts!
                self.capture_buffer.push('\n');
                self.total_chars += 1;
                debug!(
                    "Captured newline in prompt (total chars: {})",
                    self.total_chars
                );
            }

            KeyEvent::Alt => {
                // ALT key = terminator — fire the trigger!
                let raw = format!(
                    "{}{}",
                    self.config.general.trigger_prefix,
                    self.capture_buffer.trim()
                );

                let char_count = self.total_chars;

                info!(
                    "✓ Trigger complete via ALT key ({} chars to delete)",
                    char_count
                );

                if !self.capture_buffer.trim().is_empty() {
                    let trigger = CapturedTrigger {
                        raw_text: raw,
                        char_count,
                    };
                    let _ = self.tx.send(trigger);
                }

                self.reset();
            }

            KeyEvent::Escape => {
                info!("✗ Trigger cancelled by user (ESC)");
                self.reset();
            }

            KeyEvent::Backspace => {
                // User is editing their prompt — remove last char
                if self.capture_buffer.pop().is_some() {
                    self.total_chars = self.total_chars.saturating_sub(1);
                } else if self.total_chars > 0 {
                    // They deleted into the prefix — exit capture mode
                    info!("Backspace deleted trigger prefix — returning to idle");
                    self.reset();
                }
            }

            KeyEvent::Other => {
                // Arrow keys, function keys, etc. during capture — ignore
                debug!("Ignoring non-char key during capture");
            }
        }
    }

    fn reset(&mut self) {
        self.state = State::Idle;
        self.capture_buffer.clear();
        self.idle_buffer.clear();
        self.total_chars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn engine() -> (TriggerEngine, UnboundedReceiver<CapturedTrigger>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            TriggerEngine::new(Config::default(), tx, Arc::new(AppState::new())),
            rx,
        )
    }

    fn start_capture(engine: &mut TriggerEngine) {
        engine.handle_event(KeyEvent::Char('@'));
        engine.handle_event(KeyEvent::Char('@'));
    }

    #[test]
    fn captures_trigger_and_counts_prefix() {
        let (mut engine, mut rx) = engine();
        start_capture(&mut engine);
        for c in "ask hello".chars() {
            engine.handle_event(KeyEvent::Char(c));
        }
        engine.handle_event(KeyEvent::Alt);

        let captured = rx.try_recv().expect("trigger should be sent");
        assert_eq!(captured.raw_text, "//ask hello");
        assert_eq!(captured.char_count, "//ask hello".chars().count());
    }

    #[test]
    fn backspace_removes_last_captured_character() {
        let (mut engine, mut rx) = engine();
        start_capture(&mut engine);
        for c in "ask helo".chars() {
            engine.handle_event(KeyEvent::Char(c));
        }
        engine.handle_event(KeyEvent::Backspace);
        engine.handle_event(KeyEvent::Char('l'));
        engine.handle_event(KeyEvent::Alt);

        let captured = rx.try_recv().expect("trigger should be sent");
        assert_eq!(captured.raw_text, "//ask hell");
        assert_eq!(captured.char_count, "//ask hell".chars().count());
    }

    #[test]
    fn escape_cancels_without_sending() {
        let (mut engine, mut rx) = engine();
        start_capture(&mut engine);
        engine.handle_event(KeyEvent::Char('a'));
        engine.handle_event(KeyEvent::Escape);

        assert!(rx.try_recv().is_err());
        assert_eq!(engine.state, State::Idle);
    }

    #[test]
    fn enter_is_preserved_inside_prompt() {
        let (mut engine, mut rx) = engine();
        start_capture(&mut engine);
        for c in "ask first".chars() {
            engine.handle_event(KeyEvent::Char(c));
        }
        engine.handle_event(KeyEvent::Enter);
        for c in "second".chars() {
            engine.handle_event(KeyEvent::Char(c));
        }
        engine.handle_event(KeyEvent::Alt);

        let captured = rx.try_recv().expect("trigger should be sent");
        assert_eq!(captured.raw_text, "//ask first\nsecond");
    }

    #[test]
    fn empty_trigger_is_not_sent() {
        let (mut engine, mut rx) = engine();
        start_capture(&mut engine);
        engine.handle_event(KeyEvent::Alt);

        assert!(rx.try_recv().is_err());
        assert_eq!(engine.state, State::Idle);
    }

    #[test]
    fn paused_engine_ignores_input_and_resets_capture() {
        let (mut engine, mut rx) = engine();
        engine.app_state.set_paused(true);
        start_capture(&mut engine);
        for c in "ask do not send".chars() {
            engine.handle_event(KeyEvent::Char(c));
        }
        engine.handle_event(KeyEvent::Alt);

        assert!(rx.try_recv().is_err());
        assert_eq!(engine.state, State::Idle);
    }
}
