# 📖 KAI — Development Log & Architectural Deep-Dive (DEVLOG.md)

**KAI (Keyboard Artificial Intelligence)** is an OS-level universal input layer written in Rust. It intercepts a designated prefix (`//`) anywhere in Windows, captures prompt text, streams it to high-performance LLMs (Groq / Google Gemini), erases the original typed trigger using simulated backspaces, and automatically injects the generated response directly into whatever application currently holds focus.

---

## 📅 Chronological Development Timeline

Here is the chronological progression of work and architectural iterations completed on KAI:

```
[Phase 1: Inception & rdev Hooking]
       │
       ▼
[Phase 2: Win32 Native Hook Rewrite (WH_KEYBOARD_LL + GetMessageW)]
       │
       ▼
[Phase 3: Terminator Evolution (Enter ➔ Alt Key)]
       │
       ▼
[Phase 4: Multi-Provider Resilience & Gemini Auto-Fallback]
       │
       ▼
[Phase 5: Groq Cloud Integration & 120B Model Selection]
       │
       ▼
[Phase 6: Complete Removal of OpenAI Key Dependencies]
       │
       ▼
[Phase 7: Dynamic YAML-Based Triggers (triggers.yml in AppData)]
       │
       ▼
[Phase 8: Universal Trigger Prefix Transition (;; ➔ //)]
```

### 1. Inception & Hooking Challenges
- **Problem**: Initial attempts using cross-platform libraries like `rdev` failed to reliably intercept keys system-wide on 64-bit Windows without admin elevation, or silently froze when backgrounded.
- **Solution**: Rewrote the entire listening architecture in [`listener.rs`](file:///c:/Users/divya/OneDrive/Desktop/KAI/kai-core/src/listener.rs) using raw Windows API hooks (`SetWindowsHookExW(WH_KEYBOARD_LL, ...)`). Discovered that Windows low-level hooks **require** a standard Win32 message pump (`GetMessageW` / `DispatchMessageW`) running on a dedicated OS thread.

### 2. Multi-Line Prompts & Terminator Transition (`Enter` ➔ `Alt`)
- **Problem**: Using `Enter` as the terminator caused two critical issues:
  1. Users could not type multi-line prompts or paragraphs.
  2. In chat apps (Slack, Discord, WhatsApp) or search bars, pressing `Enter` prematurely submitted incomplete messages before the AI could inject its answer.
- **Solution**: Switched the capture termination key to **`Alt`**. When typing a trigger:
  - `Enter` simply inserts a newline (`\n`) into your prompt.
  - `Alt` completes the prompt, deletes the exact typed character count, and triggers AI completion.
  - `Escape` cancels capture and returns to idle.

### 3. API Resilience & Gemini Model Fallbacks
- **Problem**: Google Gemini API returned `404 NOT_FOUND` for deprecated model names (`gemini-2.0-flash`), and free tiers suffered from strict rate limits (`429 RESOURCE_EXHAUSTED`).
- **Solution**: Built [`providers/gemini.rs`](file:///c:/Users/divya/OneDrive/Desktop/KAI/kai-core/src/providers/gemini.rs) with built-in tier failovers: automatically cascading from `gemini-2.5-flash` ➔ `gemini-2.5-flash-lite` ➔ `gemini-2.5-pro`.

### 4. Groq Integration & Meta/Open-Source Power
- **Problem**: Gemini free tier limits restricted heavy daily usage.
- **Solution**: Added native support for **Groq Cloud** (`https://api.groq.com/openai/v1`), offering ultra-fast inference (500+ tokens/sec) and 14,400 free requests per day. Configured support for Meta Llama 3.3 70B and OpenAI's open-weights model `openai/gpt-oss-120b`.

### 5. Decoupling from OpenAI Keys
- **Problem**: Early configs still expected `OPENAI_API_KEY`, causing confusion.
- **Solution**: Cleaned up the codebase, configuration schemas, and launcher scripts (`start-kai.bat`, `start-kai.ps1`) to exclusively focus on **Groq** and **Gemini**.

### 6. User-Customizable Triggers via YAML (`triggers.yml`)
- **Problem**: Built-in prompts (`ask`, `coder`, `fix`, `explain`, etc.) were hardcoded into Rust source code, requiring compilation to modify.
- **Solution**: Created [`triggers.rs`](file:///c:/Users/divya/OneDrive/Desktop/KAI/kai-core/src/triggers.rs). KAI now auto-generates and reads a user-editable configuration file at `%APPDATA%\kai\triggers.yml`. Users can define custom commands, aliases, max tokens, and custom prompt templates without writing any code.

### 7. Trigger Prefix Migration (`;;` ➔ `//`)
- **Problem**: User preference for cleaner, intuitive slash notation (`//ask`, `//fix`, `//coder`).
- **Solution**: Migrated trigger prefix default from `;;` to `//` across configuration files, buffers, documentation, and the live `%APPDATA%\kai\config.toml`.

---

## ⚙️ How Everything Works (Component Breakdown)

```
[User Keystrokes] ──► [Win32 WH_KEYBOARD_LL Hook]
                             │
                             ▼
                    [TriggerEngine Buffer]
                    ├── Idle Mode (4-char rolling window)
                    └── Capturing Mode (accumulates text on "//")
                             │ (User presses ALT)
                             ▼
                    [parser.rs: Extract cmd & prompt]
                             │
                             ▼
                    [triggers.rs: Resolve prompt from YAML]
                             │
                             ▼
                    [HTTP Provider: Groq / Gemini API]
                             │
                             ▼
                    [injector.rs]
                    ├── 1. Backspace N characters (erase trigger)
                    └── 2. Keystroke / Clipboard Injection
```

### 1. `listener.rs` — Low-Level Keyboard Interception
- Uses `SetWindowsHookExW(WH_KEYBOARD_LL)` to receive global keyboard events from the Windows kernel.
- Runs a dedicated Win32 message queue thread (`GetMessageW`).
- Translates raw Virtual Key codes (`VK_MENU`, `VK_RETURN`, `0x41-0x5A`, etc.) into structured Rust `KeyEvent` enums.
- Forwards events through a lock-free thread bridge (`std::sync::mpsc` ➔ Tokio async `mpsc::unbounded_channel`).
- **Always calls `CallNextHookEx`** to ensure no system keystrokes are dropped or delayed for other applications.

### 2. `trigger.rs` — Privacy-First State Machine
- **Idle State**: Maintains a tiny rolling circular buffer (`VecDeque<char>`, max ~4-10 characters). When characters don't match `//`, old characters are immediately popped and dropped from memory.
- **Capturing State**: The moment `//` is matched at the end of the buffer, the engine switches to `State::Capturing`. It records the command, the prompt, and counts every single typed character.
- **Trigger Completion**: When `KeyEvent::Alt` is received, the captured payload is dispatched to the background Tokio runtime, and the engine resets back to `State::Idle`.

### 3. `triggers.rs` & `parser.rs` — Command Resolution
- `parser.rs` trims leading non-alphanumerics and splits `//coder write a binary search` into:
  - Command: `"coder"`
  - User Prompt: `"write a binary search"`
- `triggers.rs` reads `%APPDATA%\kai\triggers.yml` and matches `"coder"` (or aliases like `"code"`, `"dev"`) to find the exact system instructions and token limits.

### 4. `injector.rs` — Text Erasure & Smart Injection
- **Step 1 (Erase)**: Calculates total characters typed (e.g. `//ask hello` = 11 chars) and simulates 11 rapid `Backspace` clicks using the `enigo` crate.
- **Step 2 (Inject)**: Uses a dual-tier strategy:
  - **Short text (< 200 characters)**: Simulated character-by-character typing.
  - **Long text (≥ 200 characters)**: Saves current user clipboard ➔ writes AI text to clipboard ➔ simulates `Ctrl+V` ➔ waits 100ms ➔ restores original user clipboard contents.

---

## 🔒 Privacy & Safety Analysis (Is it Safe to Use?)

### What makes KAI safe:
1. **No Keylogger Disk Storage**:
   - Normal typing (outside of `//`) is **never written to disk, never logged, and never sent anywhere**.
   - It only exists in a temporary 4-character sliding window in RAM that gets overwritten with every subsequent keypress.
2. **Strict Payload Isolation**:
   - Only text typed **between `//` and the `Alt` key** is sent to the LLM API endpoint over encrypted HTTPS (TLS 1.3).
3. **Local Machine Execution**:
   - All parsing, configuration loading, and hook handling runs locally on your PC. There is no middleman telemetry server.
4. **Clipboard Integrity**:
   - When injecting large text via clipboard, KAI reads your previous clipboard contents first and restores them immediately after pasting.

---

## ⚠️ Known Loopholes, Edge Cases & Limitations

Understanding the technical trade-offs and edge cases:

| Loophole / Limitation | Explanation & Real-World Impact | Workaround / Mitigation |
|:---|:---|:---|
| **1. Admin / Elevated Windows (UIPI)** | Windows has **User Interface Privilege Isolation (UIPI)**. If you type inside an Administrator terminal (cmd/PowerShell as Admin) or Task Manager, a standard user process cannot inject keystrokes into an elevated window. | If you need KAI inside Admin terminals, right-click `start-kai.bat` and select **Run as Administrator**. |
| **2. Code Comments Collision** | In languages like C++, C#, Java, JS, and Rust, `//` is used for single-line comments. If you type `// todo: fix this` and later press `Alt` (e.g. `Alt+Tab`), KAI might think you completed a trigger called `"todo:"`. | Press `ESC` if you accidentally start typing a comment that looks like a command, or customize `trigger_prefix` in `config.toml` to a unique sequence like `;;` or `\\` if you write a lot of code comments. |
| **3. Uppercase / Shift Keys in Hook** | The low-level hook maps virtual keycodes (`0x41` ➔ `'a'`). It converts letter keys to lowercase characters by default during capture. | Prompts are processed case-insensitively. The LLM receives standard text, but exact case preservation inside the prompt text is normalized. |
| **4. Fast Typing & Backspace Lag** | In very heavy IDEs under high CPU load, backspacing 100 characters character-by-character takes ~500ms (`5ms` per backspace). If you click elsewhere during that half-second, backspaces could land in the wrong window. | Keep prompts concise, or avoid clicking to another window while the AI is erasing the prompt. |
| **5. Clipboard Race Condition** | For responses >200 characters, KAI temporarily touches the Windows clipboard for 100ms before restoring your old clipboard. | If another background clipboard manager is running, it may record the temporary AI paste snippet in clipboard history. |

---

## 🛠️ Configuration Files Reference

All local configurations are stored in your Windows user profile:

- ⚙️ **General Settings**: `%APPDATA%\kai\config.toml`
  - Change trigger prefix (`"//"`), timeout, backspace delay.
- 📝 **Trigger Customization**: `%APPDATA%\kai\triggers.yml`
  - Add new triggers, customize system prompts, edit aliases, adjust max tokens.
- 🚀 **Launcher**: `start-kai.bat`
  - Sets your `GROQ_API_KEY` or `GEMINI_API_KEY` and launches `kai.exe`.
