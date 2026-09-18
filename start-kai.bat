@echo off
:: ============================================================
:: KAI Startup Script
:: Double-click this file to start KAI!
:: ============================================================

:: Set GROQ_API_KEY or GEMINI_API_KEY in your user environment before running.
:: Never put API keys in this script or commit them to the repository.
:: Groq Model (default: openai/gpt-oss-120b)
set GROQ_MODEL=openai/gpt-oss-120b
set KAI_LOG=info
set KAI_PATH=%~dp0target\x86_64-pc-windows-gnu\release\kai.exe

if not exist "%KAI_PATH%" (
    echo ERROR: kai.exe not found at %KAI_PATH%
    pause
    exit /b 1
)

echo ============================================================
echo   KAI — Universal AI Input Layer
echo ============================================================
echo.
echo  Triggers available:
echo    @@ask   ^<question^>  - General AI assistant
echo    @@coder ^<question^>  - Programming helper
echo    @@fix   ^<text^>      - Improve writing
echo    @@explain ^<topic^>   - Clear explanation
echo    @@translate ^<text^>  - Translator
echo    @@summarize ^<text^>  - Concise summary
echo    @@email ^<text^>      - Professional email
echo    @@tweet ^<text^>      - Social media post
echo.
echo  Type trigger + prompt, then press ALT key to send!
echo  Press ESC to cancel trigger.
echo  Edit %%APPDATA%%\kai\triggers.yml to add custom triggers!
echo ============================================================
echo.

"%KAI_PATH%"
pause
