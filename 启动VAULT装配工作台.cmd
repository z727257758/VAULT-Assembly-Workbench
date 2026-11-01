@echo off
cd /d "%~dp0"
rem Prefer the newest build; a running older executable may be locked.
for /f "delims=" %%I in ('dir /b /a-d /o-d "target\portable-paths\release\vault-native*.exe" 2^>nul ^| findstr /x /c:"vault-native.exe" /c:"vault-native-spatial.exe"') do (
    start "" "target\portable-paths\release\%%I" %*
    exit /b
)
cargo run -- %*
pause
