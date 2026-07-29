@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0check.ps1"
exit /b %errorlevel%
