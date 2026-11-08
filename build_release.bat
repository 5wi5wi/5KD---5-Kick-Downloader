@echo off
title 5KD - Release EXE Builder
echo ========================================================
echo   Building 5KD.exe (Optimized Native Rust Release)
echo ========================================================
echo.

cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo [ERROR] Build failed. Ensure Rust (cargo) is installed.
    pause
    exit /b %ERRORLEVEL%
)

if not exist "release" mkdir "release"
copy /Y "target\release\5KD.exe" "release\5KD.exe" >nul

echo.
echo ========================================================
echo   SUCCESS! Standalone executable created at:
echo   %~dp0release\5KD.exe
echo ========================================================
explorer.exe "%~dp0release"
pause
