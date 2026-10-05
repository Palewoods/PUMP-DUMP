@echo off
rem Builds an optimised release of PUMP&DUMP and packs it, ready to share, into:
rem   dist\PUMP&DUMP\           the game folder (double-click PUMP&DUMP.exe)
rem   dist\PUMP-DUMP-windows.zip  the same folder, zipped
rem Names with "&" must stay inside double quotes, or cmd treats & as "and then".
setlocal
cd /d "%~dp0"
set "GAME=dist\PUMP&DUMP"
set "ZIP=dist\PUMP-DUMP-windows.zip"

cargo build --release -p pumpdump || exit /b 1

if exist "%GAME%" rmdir /s /q "%GAME%"
mkdir "%GAME%\assets"
copy /y target\release\pumpdump.exe "%GAME%\PUMP&DUMP.exe" >nul || exit /b 1
rem The game reads its tuning from assets\ next to the exe.
copy /y crates\pumpdump\assets\movement.ron "%GAME%\assets\" >nul || exit /b 1
copy /y LICENSE "%GAME%\LICENSE.txt" >nul || exit /b 1
copy /y packaging\README.txt "%GAME%\README.txt" >nul || exit /b 1

if exist "%ZIP%" del "%ZIP%"
powershell -NoProfile -Command "Compress-Archive -LiteralPath '%GAME%' -DestinationPath '%ZIP%'" || exit /b 1

echo.
echo Done: "%GAME%\PUMP&DUMP.exe" and "%ZIP%"
