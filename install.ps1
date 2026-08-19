$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot

function Refresh-Path {
    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
        [Environment]::GetEnvironmentVariable('Path', 'User')
}

function Test-Command([string]$Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Install-Winget {
    if (Test-Command winget) { return }
    Write-Host 'winget not found - installing App Installer...'
    $download = 'https://aka.ms/getwinget'
    $msix = Join-Path $env:TEMP 'winget.appx'
    Invoke-WebRequest -Uri $download -OutFile $msix
    Add-AppxPackage -Path $msix
}

Install-Winget

if (-not (Test-Command 'python')) {
    Write-Host 'Python not found - installing Python 3.12 via winget...'
    winget install --silent --accept-package-agreements --accept-source-agreements `
        --id Python.Python.3.12 --scope user
    Refresh-Path
}
if (-not (Test-Command 'python')) {
    Write-Host 'Error: Python was not found after install. Open a new terminal and rerun this script.'
    Read-Host -Prompt 'Press Enter to exit'
    exit 1
}
$pyVer = python --version
Write-Host "Using $pyVer"

if (-not (Test-Command 'cargo')) {
    Write-Host 'Rust not found - installing via winget...'
    winget install --silent --accept-package-agreements --accept-source-agreements `
        --id Rustlang.Rustup --scope User
    if (-not (Test-Path -LiteralPath "$env:USERPROFILE\.cargo\bin\cargo.exe")) {
        Write-Host 'Error: Rust was not found after install. Open a new PowerShell and rerun this script.'
        Read-Host -Prompt 'Press Enter to exit'
        exit 1
    }
    Refresh-Path
}
Write-Host "Using cargo $((cargo --version))"

$backendDir = Join-Path $PSScriptRoot 'backend'
if (-not (Test-Path -LiteralPath $backendDir)) {
    Write-Host 'Error: backend/ folder not found. Make sure install.ps1 stays next to the project files.'
    Read-Host -Prompt 'Press Enter to exit'
    exit 1
}
Write-Host 'Building the Rust backend (first build can take a few minutes)...'
cargo build --release --manifest-path (Join-Path $backendDir 'Cargo.toml')
if ($LASTEXITCODE -ne 0) {
    Write-Host 'Error: Rust build failed.'
    Read-Host -Prompt 'Press Enter to exit'
    exit 1
}
Write-Host 'Backend built.'

$guiPath = Join-Path $PSScriptRoot 'gui.py'
if (-not (Test-Path -LiteralPath $guiPath)) {
    Write-Host 'Warning: gui.py not found next to the script; skipping the desktop shortcut.'
    Read-Host -Prompt 'Press Enter to exit'
    exit 0
}

$shortcutDir = [Environment]::GetFolderPath('Desktop')
if (-not (Test-Path -LiteralPath $shortcutDir)) { $shortcutDir = $PSScriptRoot }
$shortcutPath = Join-Path $shortcutDir 'Cow Weight Estimator.lnk'
$launcher = Join-Path $PSScriptRoot 'start_gui.ps1'

$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = 'powershell.exe'
$shortcut.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$launcher`""
$shortcut.WorkingDirectory = $PSScriptRoot
$shortcut.IconLocation = 'powershell.exe,0'
$shortcut.Description = 'Cow Weight Estimator'
$shortcut.Save()
Write-Host "Desktop shortcut created: $shortcutPath"

Write-Host 'Setup complete. Double-click the shortcut to launch the app.'
Read-Host -Prompt 'Press Enter to exit'
