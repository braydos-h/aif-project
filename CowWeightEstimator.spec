# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec: one-file, windowed CowWeightEstimator.exe.

Bundles the aif package, the demo cow images (``cows/``), and the Rust
backend binary (``aif-backend.exe``) so the exe can also serve the HTTP
API. Build with::

    pyinstaller --noconfirm CowWeightEstimator.spec

or double-click ``build_exe.bat``.
"""

from pathlib import Path

REPO_ROOT = Path(SPECPATH)

backend_binary = REPO_ROOT / "backend" / "target" / "release" / "aif-backend.exe"

datas = [
    ("aif", "aif"),
    ("cows", "cows"),
]
binaries = [(str(backend_binary), "backend")] if backend_binary.is_file() else []
hiddenimports = ["PIL.Image", "PIL.ImageTk"]

a = Analysis(
    ["gui.py"],
    pathex=[str(REPO_ROOT)],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["pytest", "unittest"],
    noarchive=False,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="CowWeightEstimator",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
