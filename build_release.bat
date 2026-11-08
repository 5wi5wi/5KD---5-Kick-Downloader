@echo off
title 5KD - Release 1.0v EXE Builder
echo ========================================================
echo   Building 5KD Release 1.0v (Optimized Native Rust EXE)
echo ========================================================
echo.

cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo [ERROR] Build failed. Ensure Rust (cargo) is installed.
    pause
    exit /b %ERRORLEVEL%
)

if not exist "release\1.0v" mkdir "release\1.0v"
copy /Y "target\release\5KD.exe" "release\1.0v\5KD.exe" >nul
copy /Y "target\release\5KD.exe" "release\5KD.exe" >nul

echo.
echo ========================================================
echo   SUCCESS! Release 1.0v executable created at:
echo   %~dp0release\1.0v\5KD.exe
echo ========================================================
explorer.exe "%~dp0release\1.0v"
pause
