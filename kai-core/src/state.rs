// ============================================================
// state.rs — Shared application state
//
// Thread-safe state shared between the AI dispatcher and the
// future system tray UI. Tracks whether a request is in flight
// so the UI can show a processing indicator.
// ============================================================

use std::sync::atomic::{AtomicBool, Ordering};


/// Shared, thread-safe application state.
pub struct AppState {
    /// Whether an AI request is currently in progress.
    processing: AtomicBool,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            processing: AtomicBool::new(false),
        }
    }

    pub fn set_processing(&self, value: bool) {
        self.processing.store(value, Ordering::SeqCst);
    }

    pub fn is_processing(&self) -> bool {
        self.processing.load(Ordering::SeqCst)
    }
}
