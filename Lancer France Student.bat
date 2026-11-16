@echo off
setlocal
chcp 65001 >nul

set "EXE_PATH=%~dp0app\src-tauri\target\release\fs-code.exe"
set "ALT_EXE=%~dp0target\release\fs-code.exe"

if exist "%EXE_PATH%" (
    start "" "%EXE_PATH%"
    exit /b 0
)

if exist "%ALT_EXE%" (
    start "" "%ALT_EXE%"
    exit /b 0
)

echo ============================================================
echo         France Student Desktop - Lancement
echo ============================================================
echo.
echo [!] L'exécutable compilé (fs-code.exe) n'a pas été trouvé.
echo.
echo Deux options pour utiliser l'application :
echo.
echo 1. Télécharger directement l'installateur officiel :
echo    Rendez-vous sur les Releases GitHub :
echo    https://github.com/NoRushJustCat/FranceStudent_Desktop/releases
echo    et téléchargez le fichier d'installation (.exe ou .msi).
echo.
echo 2. Compiler vous-même depuis les sources :
echo    - Installez Node.js et Rust
echo    - Ouvrez un terminal dans le dossier 'app'
echo    - Exécutez : npm install ^&^& npm run tauri build
echo.
echo ============================================================
pause
