// ============================================================
// injector.rs — Text injection into the focused application
//
// Two-tier injection strategy:
//   1. Short text (< CLIPBOARD_THRESHOLD chars):
//      Simulate typing via enigo — character by character.
//   2. Long text (>= CLIPBOARD_THRESHOLD chars):
//      Save clipboard → write text → Ctrl+V → restore clipboard.
//      Much faster for large responses.
//
// Before injecting, sends N backspace keystrokes to erase the
// original trigger text the user typed.
// ============================================================

use anyhow::{Context, Result};
use arboard::Clipboard;
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use log::{debug, info, warn};
use std::thread;
use std::time::Duration;

/// Characters above this threshold use clipboard paste instead of keystroke simulation.
const CLIPBOARD_THRESHOLD: usize = 200;

/// Small delay between operations to let the OS process events.
const INTER_KEY_DELAY_MS: u64 = 5;

pub struct TextInjector {
    enigo: Enigo,
}

impl TextInjector {
    pub fn new() -> Self {
        let enigo = Enigo::new(&Settings::default())
            .expect("Failed to initialize Enigo input simulator");
        Self { enigo }
    }

    /// Sends N backspace keystrokes to erase the trigger text the user typed.
    ///
    /// We also need to erase the Enter keypress — but since Enter caused a
    /// newline to be inserted in most apps, we send an extra Backspace to
    /// remove it too.
    pub fn delete_chars(&mut self, char_count: usize) {
        debug!("Sending {} backspaces to erase trigger text", char_count);

        for _ in 0..char_count {
            let _ = self.enigo.key(Key::Backspace, Click);
            thread::sleep(Duration::from_millis(INTER_KEY_DELAY_MS));
        }

        thread::sleep(Duration::from_millis(30));
    }

    /// Injects text into the currently focused application.
    ///
    /// Automatically chooses between keystroke simulation (short text)
    /// or clipboard paste (long text).
    pub fn inject_text(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            warn!("inject_text called with empty string — nothing to do");
            return Ok(());
        }

        if text.len() >= CLIPBOARD_THRESHOLD {
            info!(
                "Using clipboard paste for {} chars (above threshold of {})",
                text.len(),
                CLIPBOARD_THRESHOLD
            );
            self.inject_via_clipboard(text)
        } else {
            info!("Using keystroke simulation for {} chars", text.len());
            self.inject_via_keystrokes(text)
        }
    }

    /// Types text character-by-character using keyboard simulation.
    /// Works in virtually all applications.
    fn inject_via_keystrokes(&mut self, text: &str) -> Result<()> {
        self.enigo
            .text(text)
            .context("Failed to simulate keystrokes for text injection")?;
        Ok(())
    }

    /// Pastes text via the clipboard (Ctrl+V), then restores the original clipboard.
    fn inject_via_clipboard(&mut self, text: &str) -> Result<()> {
        let mut clipboard =
            Clipboard::new().context("Failed to access system clipboard")?;

        // Save original clipboard contents
        let original = clipboard.get_text().ok(); // None if clipboard is empty or non-text

        // Write our text to clipboard
        clipboard
            .set_text(text)
            .context("Failed to write to clipboard")?;

        // Small delay to ensure clipboard is ready
        thread::sleep(Duration::from_millis(50));

        // Simulate Ctrl+V
        self.enigo.key(Key::Control, Press)?;
        thread::sleep(Duration::from_millis(10));
        self.enigo.key(Key::Unicode('v'), Click)?;
        thread::sleep(Duration::from_millis(10));
        self.enigo.key(Key::Control, Release)?;

        // Wait for paste to complete
        thread::sleep(Duration::from_millis(100));

        // Restore original clipboard contents
        match original {
            Some(orig) => {
                let _ = clipboard.set_text(&orig);
                debug!("Original clipboard contents restored");
            }
            None => {
                // Clear clipboard (set to empty) if it was originally empty
                // arboard doesn't have a clear method, so we set empty string
                let _ = clipboard.set_text("");
            }
        }

        Ok(())
    }
}
