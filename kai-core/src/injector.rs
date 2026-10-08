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

pub struct TextInjector {
    enigo: Enigo,
    backspace_delay_ms: u64,
    clipboard_threshold: usize,
}

impl TextInjector {
    pub fn new(backspace_delay_ms: u64, clipboard_threshold: usize) -> Self {
        let enigo =
            Enigo::new(&Settings::default()).expect("Failed to initialize Enigo input simulator");
        Self {
            enigo,
            backspace_delay_ms,
            clipboard_threshold,
        }
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
            thread::sleep(Duration::from_millis(self.backspace_delay_ms));
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

        if should_use_clipboard(text, self.clipboard_threshold) {
            info!(
                "Using clipboard paste for {} chars (above threshold of {})",
                text.len(),
                self.clipboard_threshold
            );
            self.inject_via_clipboard(text)
        } else {
            info!("Using keystroke simulation for {} chars", text.len());
            self.inject_via_keystrokes(text)
        }
    }

    /// Copies text to the clipboard as a fallback when the focused
    /// application rejects simulated typing or paste injection.
    pub fn copy_to_clipboard(&self, text: &str) -> Result<()> {
        let mut clipboard = Clipboard::new().context("Failed to access system clipboard")?;
        clipboard
            .set_text(text)
            .context("Failed to copy response to clipboard")?;
        info!("Response copied to clipboard as injection fallback");
        Ok(())
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
        let mut clipboard = Clipboard::new().context("Failed to access system clipboard")?;

        // Save original clipboard contents
        let original = clipboard.get_text().ok(); // None if clipboard is empty or non-text

        // Keep the paste operation inside a closure so clipboard restoration
        // runs even when writing or simulating Ctrl+V returns an error.
        let paste_result = (|| -> Result<()> {
            clipboard
                .set_text(text)
                .context("Failed to write to clipboard")?;

            // Small delay to ensure clipboard is ready
            thread::sleep(Duration::from_millis(50));

            // Simulate Ctrl+V
            self.enigo.key(Key::Control, Press)?;
            thread::sleep(Duration::from_millis(10));
            let paste_key_result = self.enigo.key(Key::Unicode('v'), Click);
            thread::sleep(Duration::from_millis(10));
            let release_result = self.enigo.key(Key::Control, Release);
            paste_key_result?;
            release_result?;

            // Wait for paste to complete
            thread::sleep(Duration::from_millis(100));
            Ok(())
        })();

        // Restore original clipboard contents. If the clipboard was not text,
        // arboard cannot round-trip its formats safely. Leaving the generated
        // response in place is safer than silently replacing an image/file
        // clipboard with an empty string.
        match original {
            Some(orig) => {
                let _ = clipboard.set_text(&orig);
                debug!("Original clipboard contents restored");
            }
            None => {
                warn!("Original clipboard was empty or non-text; leaving response in clipboard because its original format cannot be restored safely");
            }
        }

        paste_result
    }
}

fn should_use_clipboard(text: &str, threshold: usize) -> bool {
    text.chars().count() >= threshold
}

#[cfg(test)]
mod tests {
    use super::should_use_clipboard;

    #[test]
    fn uses_clipboard_at_or_above_threshold() {
        assert!(!should_use_clipboard("abcd", 5));
        assert!(should_use_clipboard("abcde", 5));
        assert!(should_use_clipboard("abcdef", 5));
    }

    #[test]
    fn threshold_counts_unicode_characters() {
        assert!(!should_use_clipboard("éé", 3));
        assert!(should_use_clipboard("ééé", 3));
    }
}
