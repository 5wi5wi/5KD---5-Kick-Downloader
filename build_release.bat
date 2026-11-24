@echo off
title 5KD - Release Installer Builder
echo ========================================================
echo   Building 5KD Release (Native Rust + Setup Installer)
echo ========================================================
echo.

cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo [ERROR] Cargo build failed. Ensure Rust (cargo) is installed.
    pause
    exit /b %ERRORLEVEL%
)

if not exist "dist" mkdir "dist"

set "ISCC_PATH=C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
if exist "%ISCC_PATH%" (
    echo.
    echo Compiling Windows Setup Wizard Installer...
    "%ISCC_PATH%" "installer.iss" "/DMySourceExe=target\release\5KD.exe" "/DMyOutputDir=dist" "/DMyOutputBaseFilename=5KD-windows-x86_64-setup"
    echo.
    echo ========================================================
    echo   SUCCESS! Windows Setup Wizard Installer created at:
    echo   %~dp0dist\5KD-windows-x86_64-setup.exe
    echo ========================================================
    explorer.exe "%~dp0dist"
) else (
    copy /Y "target\release\5KD.exe" "dist\5KD.exe" >nul
    echo.
    echo [INFO] Inno Setup 6 not found at "%ISCC_PATH%".
    echo   Copied compiled binary to: %~dp0dist\5KD.exe
    explorer.exe "%~dp0dist"
)
pause
