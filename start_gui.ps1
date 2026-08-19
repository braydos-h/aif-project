$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot

$guiPath = Join-Path $PSScriptRoot 'gui.py'
if (-not (Test-Path -LiteralPath $guiPath)) {
    Write-Host "Error: gui.py not found in `"$PSScriptRoot`""
    Write-Host 'Make sure the script stays next to the project files.'
    Read-Host -Prompt 'Press Enter to exit'
    exit 1
}

$pythonw = Get-Command pythonw.exe -ErrorAction SilentlyContinue
if ($pythonw) {
    Start-Process -FilePath $pythonw.Source -ArgumentList $guiPath
    exit 0
}

$python = Get-Command python.exe -ErrorAction SilentlyContinue
if ($python) {
    Start-Process -FilePath $python.Source -ArgumentList $guiPath
    exit 0
}

Write-Host 'Error: Python was not found on PATH.'
Write-Host 'Install Python 3.8+ from https://www.python.org/downloads/'
Write-Host 'and make sure "Add python.exe to PATH" is checked.'
Read-Host -Prompt 'Press Enter to exit'
exit 1
