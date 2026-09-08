@echo off
setlocal
cd /d "%~dp0"

set "BACKEND=%~dp0backend\target\release\aif-backend.exe"
if not exist "%BACKEND%" (
    echo Error: backend binary not found at "%BACKEND%"
    echo Build it with: cargo build --release --manifest-path backend\Cargo.toml
    pause
    exit /b 1
)

start "Cow Weight Estimator WebUI" "%BACKEND%" --host 127.0.0.1 --port 8080
start "" "http://127.0.0.1:8080/"
exit /b 0
