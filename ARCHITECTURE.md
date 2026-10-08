# KAI Architecture

KAI is organized as a pipeline with replaceable edges:

```text
Platform input
    -> Listener
    -> Trigger engine
    -> Parser
    -> Request builder
    -> Provider factory / AI provider
    -> Response policy
    -> Injector
    -> Focused application
```

## Responsibilities

### Platform boundary

`listener.rs` and `injector.rs` contain Windows-specific keyboard and text
integration. They should expose small interfaces to the rest of the program.
Future Linux and macOS support belongs in sibling platform adapters, not in the
trigger or provider logic.

### Input pipeline

`listener.rs` emits normalized `KeyEvent` values. `trigger.rs` owns the state
machine and produces a `CapturedTrigger`. `parser.rs` converts that value into
a command and prompt. None of these modules should know which AI vendor is
being used.

### Application pipeline

The dispatcher validates the parsed command, applies the configured trigger
policy, builds a provider request, handles timeouts/errors, and applies the
response through the injector. This is the future home for context selection,
conversation memory, cancellation, retries, and response editing.

### Provider boundary

`providers::AiProvider` is the stable interface for cloud, compatible, local,
and vision providers. `providers::factory` is the only place that maps config
to a concrete provider. New providers should implement the trait and register
themselves there without changing trigger handling.

### Configuration boundary

`config.rs` owns file locations, defaults, deserialization, and validation.
Secrets come from the environment or a future OS credential-store adapter.
Runtime modules should receive validated configuration rather than reading
environment variables directly.

## Extension rules

1. Keep vendor names out of the trigger and injection layers.
2. Keep OS APIs out of provider and parser modules.
3. Add new request capabilities as optional data, so text-only providers keep
   working when vision or files are introduced.
4. Return typed errors at boundaries; do not silently fall back to unrelated
   behavior.
5. Keep prompt contents and credentials out of logs.
6. Add unit tests for each state machine and provider adapter, plus one
   end-to-end pipeline test using a fake provider and fake injector.

## Planned additions

- `context.rs`: selected text, active application, and conversation history.
- `attachments.rs`: image/file metadata and secure temporary-file handling.
- `routing.rs`: provider capabilities, model selection, retries, and fallback.
- `platform/`: Windows, Linux, and macOS listener/injector adapters.
- `ui/`: tray status, pause controls, and settings without changing the core.
