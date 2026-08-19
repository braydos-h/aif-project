@echo off
setlocal
cd /d "%~dp0"

echo === CowWeightEstimator.exe build ===
echo.

REM ---- Locate Python: python, then the py launcher, then python3 ----
set "PY="
where python >nul 2>nul
if %errorlevel%==0 (
    set "PY=python"
) else (
    where py >nul 2>nul
    if %errorlevel%==0 (
        set "PY=py -3"
    ) else (
        where python3 >nul 2>nul
        if %errorlevel%==0 set "PY=python3"
    )
)
if not defined PY (
    echo Error: Python was not found on PATH.
    echo Install Python 3.8+ from https://www.python.org/downloads/
    echo and make sure "Add python.exe to PATH" is checked.
    goto :fail
)
%PY% --version
if errorlevel 1 goto :fail

REM ---- Build the Rust backend binary (needed by the spec file). ----
REM If Rust is missing but a release binary already exists, keep going
REM with that one. Set SKIP_RUST=1 to skip this step entirely.
if /i "%SKIP_RUST%"=="1" (
    echo SKIP_RUST=1 set; skipping the Rust build.
    if not exist "backend\target\release\aif-backend.exe" (
        echo Warning: no backend\target\release\aif-backend.exe found; the exe
        echo will be built without the HTTP backend.
    )
    goto :rust_done
)

set "CARGO="
where cargo >nul 2>nul
if %errorlevel%==0 (
    set "CARGO=cargo"
) else if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
    set "CARGO=%USERPROFILE%\.cargo\bin\cargo.exe"
)

if defined CARGO (
    call "%CARGO%" build --release --manifest-path backend\Cargo.toml
    if errorlevel 1 (
        if exist "backend\target\release\aif-backend.exe" (
            echo Warning: cargo build failed; reusing existing backend\target\release\aif-backend.exe
        ) else (
            goto :fail
        )
    )
    if not exist "backend\target\release\aif-backend.exe" (
        echo Error: cargo build finished but backend\target\release\aif-backend.exe
        echo was not produced.
        goto :fail
    )
) else if exist "backend\target\release\aif-backend.exe" (
    echo cargo not found; reusing existing backend\target\release\aif-backend.exe
) else (
    echo cargo not found and no existing backend binary. Install Rust from
    echo https://rustup.rs or build it manually:
    echo     cargo build --release --manifest-path backend\Cargo.toml
    goto :fail
)
:rust_done

REM ---- Make sure the spec file is present ----
if not exist "CowWeightEstimator.spec" (
    echo Error: CowWeightEstimator.spec not found next to this script.
    goto :fail
)

REM ---- Warn if the previous build is still running (it locks the exe) ----
tasklist /FI "IMAGENAME eq CowWeightEstimator.exe" 2>nul | find /i "CowWeightEstimator.exe" >nul
if %errorlevel%==0 (
    echo Warning: CowWeightEstimator.exe is currently running. Close it first,
    echo otherwise PyInstaller cannot overwrite dist\CowWeightEstimator.exe.
    set /p "CONTINUE=Press Enter to continue anyway, or Ctrl+C to abort..."
)

REM ---- Make sure PyInstaller is available ----
%PY% -m PyInstaller --version >nul 2>nul
if errorlevel 1 (
    echo Installing PyInstaller...
    %PY% -m pip install pyinstaller
    if errorlevel 1 (
        echo pip install failed; trying ensurepip first...
        %PY% -m ensurepip --upgrade
        if errorlevel 1 goto :fail
        %PY% -m pip install pyinstaller
        if errorlevel 1 goto :fail
    )
)

REM ---- Run PyInstaller (pass "clean" as first arg to clear its cache) ----
set "CLEAN="
if /i "%~1"=="clean" set "CLEAN=--clean"
%PY% -m PyInstaller --noconfirm %CLEAN% CowWeightEstimator.spec
if errorlevel 1 goto :fail

if not exist "dist\CowWeightEstimator.exe" (
    echo Error: build finished but dist\CowWeightEstimator.exe was not produced.
    goto :fail
)

echo.
echo Done. Your executable is: dist\CowWeightEstimator.exe
for %%F in ("dist\CowWeightEstimator.exe") do echo Size: %%~zF bytes
exit /b 0

:fail
echo.
echo Build failed. See the messages above.
pause
exit /b 1
