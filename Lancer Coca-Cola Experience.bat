@echo off
setlocal
chcp 65001 >nul
echo Lancement de l'expérience Coca-Cola...
start "" "%~dp0coca-cola-showcase\index.html"
exit /b 0
