// ============================================================
// state.rs — Shared application state
//
// Thread-safe state shared between the AI dispatcher and the
// future system tray UI. Tracks whether a request is in flight
// so the UI can show a processing indicator.
// ============================================================

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::providers::ConversationMessage;

/// Shared, thread-safe application state.
pub struct AppState {
    /// Whether an AI request is currently in progress.
    processing: AtomicBool,
    /// Whether KAI should ignore keyboard input until resumed.
    paused: AtomicBool,
    history: Mutex<VecDeque<ConversationMessage>>,
    history_limit: usize,
}

#[cfg(test)]
mod tests {
    use super::AppState;

    #[test]
    fn history_is_bounded_by_configured_turn_limit() {
        let state = AppState::with_history_limit(2);
        for index in 0..4 {
            state.remember_turn(format!("prompt-{index}"), format!("response-{index}"));
        }

        let history = state.conversation_history();
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].content, "prompt-2");
        assert_eq!(history[3].content, "response-3");
    }
}

impl AppState {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_history_limit(10)
    }

    pub fn with_history_limit(history_limit: usize) -> Self {
        Self {
            processing: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            history: Mutex::new(VecDeque::new()),
            history_limit,
        }
    }

    pub fn set_processing(&self, value: bool) {
        self.processing.store(value, Ordering::SeqCst);
    }

    pub fn is_processing(&self) -> bool {
        self.processing.load(Ordering::SeqCst)
    }

    pub fn toggle_paused(&self) -> bool {
        let was_paused = self.paused.fetch_xor(true, Ordering::SeqCst);
        !was_paused
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn conversation_history(&self) -> Vec<ConversationMessage> {
        self.history
            .lock()
            .map(|history| history.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn remember_turn(&self, user_prompt: String, response: String) {
        if let Ok(mut history) = self.history.lock() {
            history.push_back(ConversationMessage {
                role: "user".to_string(),
                content: user_prompt,
            });
            history.push_back(ConversationMessage {
                role: "assistant".to_string(),
                content: response,
            });
            while history.len() > self.history_limit * 2 {
                history.pop_front();
            }
        }
    }
}
