@echo off
setlocal
cd /d "%~dp0"

echo === CowWeightEstimator.exe build ===
echo.

REM Build the Rust backend binary (needed by the spec file). If Rust is
REM missing but a release binary already exists, keep going with that one.
where cargo >nul 2>nul
if %errorlevel%==0 (
    call cargo build --release --manifest-path backend\Cargo.toml
    if errorlevel 1 goto :fail
) else if exist "backend\target\release\aif-backend.exe" (
    echo cargo not found; reusing existing backend\target\release\aif-backend.exe
) else (
    echo cargo not found and no existing backend binary. Install Rust from
    echo https://rustup.rs or build it manually:
    echo     cargo build --release --manifest-path backend\Cargo.toml
    goto :fail
)

REM Make sure PyInstaller is available.
python -m PyInstaller --version >nul 2>nul
if errorlevel 1 (
    echo Installing PyInstaller...
    python -m pip install pyinstaller
    if errorlevel 1 goto :fail
)

python -m PyInstaller --noconfirm CowWeightEstimator.spec
if errorlevel 1 goto :fail

echo.
echo Done. Your executable is: dist\CowWeightEstimator.exe
exit /b 0

:fail
echo.
echo Build failed. See the messages above.
pause
exit /b 1
