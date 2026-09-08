$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot

$backend = Join-Path $PSScriptRoot 'backend\target\release\aif-backend.exe'
if (-not (Test-Path -LiteralPath $backend)) {
    Write-Host "Error: backend binary not found at `"$backend`""
    Write-Host 'Build it with: cargo build --release --manifest-path backend\Cargo.toml'
    Read-Host -Prompt 'Press Enter to exit'
    exit 1
}

Start-Process -FilePath $backend -ArgumentList '--host', '127.0.0.1', '--port', '8080'
Start-Process -FilePath 'http://127.0.0.1:8080/'
