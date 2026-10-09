# KAI — Universal AI Input Layer

> Type AI prompts anywhere on your computer. Get responses injected directly where you're typing.

KAI is a lightweight background utility that runs quietly in the background and intercepts special trigger patterns you type in **any application** — browsers, VS Code, Word, Notion, terminals, chat apps — and replaces them with AI-generated responses.

## How it works

Type a trigger pattern anywhere, press Alt, and the response appears in place:

```
//ask what is the difference between TCP and UDP

→ TCP is connection-oriented and guarantees delivery. UDP is connectionless
  and faster but doesn't guarantee delivery. TCP is used for web browsing
  and file transfers; UDP is used for streaming and gaming.
```

```
//fix I wanted to ask if you could send me the report tomorrow

→ Could you please send me the report tomorrow?
```

```
//coder explain why this is O(n²) complexity: for i in arr: for j in arr: print(i,j)

→ This is O(n²) because for each element i in arr (n iterations), you loop
  through all elements j in arr again (n iterations), giving n × n = n² total
  operations. This is called a nested loop pattern...
```

## Built-in Commands

| Trigger | Purpose | System Prompt |
|:---|:---|:---|
| `//ask` | General AI assistant | Helpful, concise answers |
| `//coder` | Programming assistant | Expert coding help with examples |
| `//fix` | Writing editor | Improve grammar, clarity, tone |
| `//explain` | Explainer | Simplified explanations with analogies |
| `//translate` | Translator | English ↔ Spanish (auto-detects) |
| `//summarize` | Summarizer | 2–3 sentence summaries |
| `//email` | Email writer | Professional business emails |
| `//tweet` | Social media | Engaging, concise posts |

## Setup

### Prerequisites
- Windows 10/11 (macOS and Linux support coming in Phase 5)
- An Groq key ([get one here](https://console.groq.com/keys))

### Install
```powershell
# Clone the repository
git clone https://github.com/yourusername/kai

# Enter the directory
cd kai

# Build the release binary
cargo build --release
```

### Run

```powershell
# Set your API key
$env:GROQ_API_KEY = "[ENCRYPTION_KEY]"

# Start KAI
.\target\x86_64-pc-windows-gnu\release\kai.exe
```

Or create a `.env` style script:

```powershell
# start-kai.ps1
$env:GROQ_API_KEY = "[ENCRYPTION_KEY]"
.\target\x86_64-pc-windows-gnu\release\kai.exe
```

### Configuration

On first run, KAI creates a config file at:
- **Windows:** `%APPDATA%\kai\config.toml`
- **macOS:** `~/Library/Application Support/kai/config.toml`
- **Linux:** `~/.config/kai/config.toml`

Edit it to change models, base URLs, or injection settings.

When KAI is running from a terminal, the control channel currently accepts:

- `p` — pause or resume capture and AI requests
- `s` — show active/paused and processing status
- `q` — quit KAI

The system tray also provides **Configure KAI**, which opens the validated
settings flow, and **Open configuration** for direct advanced editing.

These controls use the same shared state that the future system-tray UI will
use, so the UI can be added without changing the trigger pipeline.

The provider layer is intentionally extensible. `active_provider = "auto"`
keeps the current Groq/Gemini behavior. The generic `openai-compatible`
option is reserved for compatible cloud endpoints and future local Qwen,
Ollama, or LM Studio backends. Vision and file input can later be added to
the provider request model without changing trigger detection or text
injection.

## Architecture

```
kai-core/
├── src/
│   ├── main.rs         # Entry point, task orchestration
│   ├── listener.rs     # Global keyboard listener (rdev)
│   ├── trigger.rs      # Rolling buffer & state machine
│   ├── parser.rs       # "//command prompt" → ParsedCommand
│   ├── injector.rs     # Text injection (enigo + arboard)
│   ├── config.rs       # TOML config loading
│   ├── state.rs        # Shared processing state
│   ├── tray.rs         # System-tray controls and config launcher
│   └── providers/
│       ├── mod.rs      # AiProvider trait and shared request types
│       ├── factory.rs  # Config → provider boundary
│       ├── openai.rs   # OpenAI-compatible implementation
│       └── gemini.rs   # Gemini implementation
```

## Roadmap

- **Phase 0** ✅ — Input stabilization, error handling, validation, and regression tests
- **Phase 1** ✅ — Configuration foundation and validation
- **Phase 2** ✅ — Core loop and custom commands via `triggers.yml`
- **Phase 3** — Multi-provider foundation: Gemini and OpenAI-compatible endpoints
- **Phase 4** — System tray controls (in progress) + settings window + OS keyring
- **Phase 5** — Context awareness (selected text, active app)
- **Phase 6** — Streaming, model routing, local models

To save credentials securely without putting them in a project file, run:

```powershell
.\target\x86_64-pc-windows-gnu\release\kai.exe --setup-credentials
```

To edit validated KAI settings interactively, run:

```powershell
.\target\x86_64-pc-windows-gnu\release\kai.exe --configure
```

## Privacy & Security

- KAI's rolling buffer is max ~10 characters — only enough to detect `//`
- No keystrokes are persisted to disk in Idle mode
- Full prompt capture only activates **after** a valid trigger is matched
- The last 10 conversation turns are kept in memory only and are not written to disk
- API keys must be supplied through environment variables or OS credential storage; never commit them
- Prompt contents are not written to logs
- All AI requests go over HTTPS
- No telemetry

See [SECURITY.md](SECURITY.md) for the keyboard-hook privacy model and
credential-handling policy.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the pipeline boundaries and rules
for adding providers, local models, vision, context, and other operating
systems.

## License

MIT
