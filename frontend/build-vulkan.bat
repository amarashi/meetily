@echo off
REM Meetily Vulkan GPU build (daily driver on this machine: RTX 5080).
REM Mirrors build-gpu.bat's env setup but forces the Vulkan feature instead of
REM relying on auto-detect (which short-circuits to CPU when CUDA isn't present).
setlocal enabledelayedexpansion

REM Run from this script's own directory (frontend) regardless of caller cwd.
cd /d "%~dp0"

echo ========================================
echo   Meetily Vulkan GPU Build
echo ========================================

REM whisper-rs-sys bindgen needs LLVM 17 (NOT latest) or it mis-parses structs.
set "LIBCLANG_PATH=C:\Program Files\LLVM\bin"

echo Setting up Visual Studio 2022 Build Tools environment...
if exist "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" (
    call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
) else (
    echo ERROR: VS 2022 BuildTools vcvars64.bat not found
    exit /b 1
)

REM Prepend toolchain + Vulkan Bin (glslc) + fnm-managed Node/pnpm to PATH.
REM (fnm activates Node per-shell; a bare cmd doesn't see it, so add it explicitly.)
set "FNM_NODE_DIR=%APPDATA%\fnm\node-versions\v24.18.0\installation"
set "PATH=%USERPROFILE%\.cargo\bin;C:\Program Files\CMake\bin;C:\Program Files\LLVM\bin;%VULKAN_SDK%\Bin;%FNM_NODE_DIR%;%PATH%"

echo LIBCLANG_PATH=%LIBCLANG_PATH%
echo VULKAN_SDK=%VULKAN_SDK%

REM --- Build llama-helper sidecar (release, CPU) and stage it ---
REM NOTE: the sidecar is built plain (no GPU feature). llama.cpp's Vulkan path
REM needs CMake's VS generator to compile vulkan-shaders-gen, which fails here
REM (VCEnd / no CXX compiler). GPU acceleration is applied to the main app's
REM whisper-rs via tauri:build:vulkan, not to this LLM sidecar.
echo.
echo Building llama-helper sidecar (release, CPU)...
pushd "..\llama-helper"
call cargo build --release
if errorlevel 1 (
    echo ERROR: llama-helper build failed
    popd
    exit /b 1
)
popd

for /f "tokens=2" %%i in ('rustc -vV ^| findstr "host:"') do set "TARGET_TRIPLE=%%i"
echo Target triple: !TARGET_TRIPLE!

set "BINARIES_DIR=src-tauri\binaries"
if not exist "%BINARIES_DIR%" mkdir "%BINARIES_DIR%"
del /q "%BINARIES_DIR%\llama-helper*" 2>nul
copy /Y "..\target\release\llama-helper.exe" "%BINARIES_DIR%\llama-helper-!TARGET_TRIPLE!.exe" >nul
if errorlevel 1 (
    echo ERROR: failed to stage llama-helper sidecar
    exit /b 1
)
echo Staged sidecar: %BINARIES_DIR%\llama-helper-!TARGET_TRIPLE!.exe

REM --- Build the Tauri app with Vulkan acceleration ---
echo.
echo Building Tauri app (tauri:build:vulkan)...
call pnpm run tauri:build:vulkan
if errorlevel 1 (
    echo ERROR: tauri build failed
    exit /b 1
)

echo.
echo ========================================
echo   Build completed successfully
echo ========================================
exit /b 0
