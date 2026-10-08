# Security Policy

## API keys

Never place Groq, Gemini, or any other API key in source files, startup
scripts, configuration committed to Git, logs, screenshots, or issue reports.
Use `GROQ_API_KEY` or `GEMINI_API_KEY` environment variables. Any key that was
ever committed must be revoked and replaced; deleting it from the latest file
does not remove it from Git history or backups.

## Keyboard and privacy model

KAI uses a Windows global keyboard hook to detect its configured trigger. In
the current Windows MVP, users should pause or exit KAI before entering
passwords, payment information, recovery codes, or other secrets. KAI does
not intentionally persist idle keystrokes and does not log prompt contents.

Only text following a detected trigger is sent to the selected AI provider.
Users should review their provider's data-retention policy before using cloud
models. Local providers are recommended for confidential source code.

## Reporting a vulnerability

Do not open a public issue containing credentials or private prompt data.
Report security issues privately to the project maintainers and include the
affected version, reproduction steps, and a minimal sanitized example.
