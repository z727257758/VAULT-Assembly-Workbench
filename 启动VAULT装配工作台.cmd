@echo off
cd /d "%~dp0"
if exist "target\portable-paths\release\vault-native.exe" (
    start "" "target\portable-paths\release\vault-native.exe" %*
    exit /b
)
cargo run -- %*
pause
