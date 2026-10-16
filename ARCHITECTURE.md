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

`main.rs` validates the parsed command, resolves trigger instructions and
optional provider/model overrides, captures configured selected-text context,
uses bounded in-memory conversation history, applies request timeouts, and
passes the response to the injector. Response editing and cancellation remain
future work.

### Provider boundary

`providers::AiProvider` is the stable interface for cloud, compatible, local,
and future vision providers. `providers::factory` is the only place that maps
config to a concrete provider. `providers::catalog` supplies model IDs and
capability evidence to the settings UI. It distinguishes advertised support
from unknown metadata; unknown must be probed rather than treated as supported.
New providers should implement the trait and register themselves there without
changing trigger handling.

### Configuration boundary

`config.rs` owns file locations, defaults, deserialization, migration, and
validation. `credentials.rs` stores provider keys in Windows Credential
Manager; environment variables can override those saved keys. Provider
environment overrides are resolved at the provider-factory boundary rather
than in listener or trigger code. Other operating systems need their own
credential-store adapters.

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

- Image/file request data and provider adapters for vision-capable models.
- Linux and macOS listener/injector and credential-store adapters.
- Cross-platform settings UI and installer/package workflows.
- Explicit cancellation and response-editing flows.
