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
.\target\release\kai.exe
```

Or create a `.env` style script:

```powershell
# start-kai.ps1
$env:GROQ_API_KEY = "[ENCRYPTION_KEY]"
.\target\release\kai.exe
```

### Configuration

On first run, KAI creates a config file at:
- **Windows:** `%APPDATA%\kai\config.toml`
- **macOS:** `~/Library/Application Support/kai/config.toml`
- **Linux:** `~/.config/kai/config.toml`

Edit it to change models, base URLs, or injection settings.

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
│   └── providers/
│       ├── mod.rs      # AiProvider trait
│       └── openai.rs   # OpenAI implementation
```

## Roadmap

- **Phase 1** ✅ — Core loop: detect → call AI → inject
- **Phase 2** — Custom commands via config file
- **Phase 3** — Multi-provider: Anthropic, Gemini, Ollama
- **Phase 4** — System tray UI + settings window + OS keyring
- **Phase 5** — Context awareness (selected text, active app)
- **Phase 6** — Streaming, model routing, local models

## Privacy & Security

- KAI's rolling buffer is max ~10 characters — only enough to detect `//`
- No keystrokes are persisted to disk in Idle mode
- Full prompt capture only activates **after** a valid trigger is matched
- API keys are never logged or stored in plain text (use env var or OS keyring)
- All AI requests go over HTTPS
- No telemetry

## License

MIT
