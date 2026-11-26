@echo off
rem Finds where the game keeps the clock TEXT it draws (e.g. "17:02"). Double-click: it asks for the time exactly as the
rem pause screen shows it; the first run is a full scan, about 7 min, later runs narrow the survivors, instant.
cd /d "%~dp0..	argetelease"
set /p T=Clock text exactly as shown, e.g. 17:02:
if exist text.txt goto again
echo No full scan yet: scanning the whole game for "%T%", about 7 minutes...
vi-rpc.exe text %T%
goto end
:again
echo Narrowing the candidates with "%T%"...
vi-rpc.exe text --again %T%
:end
echo.
pause
