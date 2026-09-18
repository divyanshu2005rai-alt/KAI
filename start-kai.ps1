# KAI Startup Script (PowerShell)
# ============================================================
# Set your Groq or Gemini API key and run this script to start KAI.
# Usage: .\start-kai.ps1
# ============================================================

# ── Configuration ─────────────────────────────────────────
$GroqApiKey   = $env:GROQ_API_KEY
$GeminiApiKey = $env:GEMINI_API_KEY

# Log level: info (default) | debug (verbose) | warn (quiet)
$LogLevel = "info"

# Path to the KAI binary
$KaiBinary = Join-Path $PSScriptRoot "target\x86_64-pc-windows-gnu\debug\kai.exe"
# ──────────────────────────────────────────────────────────

# Validate setup
if (-not (Test-Path $KaiBinary)) {
    Write-Host "ERROR: kai.exe not found at $KaiBinary" -ForegroundColor Red
    Write-Host "Build it first with: cargo +stable-x86_64-pc-windows-gnu build" -ForegroundColor Yellow
    exit 1
}

if (-not $env:GROQ_API_KEY -and -not $env:GEMINI_API_KEY) {
    Write-Host "ERROR: No API key found." -ForegroundColor Red
    Write-Host "Please set `$GroqApiKey or `$GeminiApiKey in start-kai.ps1, or set GROQ_API_KEY / GEMINI_API_KEY environment variable." -ForegroundColor Yellow
    exit 1
}

$env:KAI_LOG = $LogLevel

Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║          KAI — Universal AI Input Layer                  ║" -ForegroundColor Cyan
Write-Host "╠══════════════════════════════════════════════════════════╣" -ForegroundColor Cyan
Write-Host "║  Type any of these triggers in ANY application:          ║" -ForegroundColor White
Write-Host "║                                                           ║" -ForegroundColor White
Write-Host "║  ;;ask   <question>     →  General AI assistant          ║" -ForegroundColor Green
Write-Host "║  ;;coder <question>     →  Programming assistant         ║" -ForegroundColor Green
Write-Host "║  ;;fix   <text>         →  Improve writing               ║" -ForegroundColor Green
Write-Host "║  ;;explain <topic>      →  Simple explanation            ║" -ForegroundColor Green
Write-Host "║  ;;translate <text>     →  English ↔ Spanish             ║" -ForegroundColor Green
Write-Host "║  ;;summarize <text>     →  Concise summary               ║" -ForegroundColor Green
Write-Host "║  ;;email <text>         →  Professional email            ║" -ForegroundColor Green
Write-Host "║  ;;tweet <text>         →  Social media post             ║" -ForegroundColor Green
Write-Host "║                                                           ║" -ForegroundColor White
Write-Host "║  Press ALT to send prompt. Press ESC to cancel.          ║" -ForegroundColor Yellow
Write-Host "║  Press Ctrl+C in this window to quit KAI.                ║" -ForegroundColor Yellow
Write-Host "╚══════════════════════════════════════════════════════════╝" -ForegroundColor Cyan
Write-Host ""

# Launch KAI
& $KaiBinary
