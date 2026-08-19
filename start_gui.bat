@echo off
setlocal
cd /d "%~dp0"

if not exist "gui.py" (
    echo Error: gui.py not found in "%~dp0"
    echo Make sure the batch file stays next to the project files.
    pause
    exit /b 1
)

set "PYTHONW="
for %%P in (pythonw.exe) do set "PYTHONW=%%~$PATH:P"
if defined PYTHONW (
    start "Cow Weight Estimator" "%PYTHONW%" "%~dp0gui.py"
    exit /b 0
)

set "PYTHON="
for %%P in (python.exe) do set "PYTHON=%%~$PATH:P"
if defined PYTHON (
    start "Cow Weight Estimator" "%PYTHON%" "%~dp0gui.py"
    exit /b 0
)

echo Error: Python was not found on PATH.
echo Install Python 3.8+ from https://www.python.org/downloads/
echo and make sure "Add python.exe to PATH" is checked.
pause
exit /b 1
