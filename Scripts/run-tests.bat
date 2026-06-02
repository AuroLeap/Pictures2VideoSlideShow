@echo off
REM Local test/quality harness (batch wrapper around run-tests.ps1).
REM Usage: scripts\run-tests.bat
pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-tests.ps1" %*
if errorlevel 1 (
    echo.
    echo Harness failed.
    exit /b 1
)
