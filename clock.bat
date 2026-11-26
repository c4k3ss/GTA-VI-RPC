@echo off
rem Finds where the game keeps its clock. Just double-click it: it asks for the in-game time and decides by itself
rem whether this is the first full scan, about 7 min, or a narrowing pass, instant. Run it again each time the clock moved.
cd /d D:\Portfolio\gta6-rpc\target-scan\release
set /p HOUR=In-game hour, e.g. 13:
set /p MIN=In-game minute, e.g. 25:
if exist clock.txt goto again
echo No full scan yet: scanning the whole game for %HOUR%:%MIN%, about 7 minutes...
vi-rpc.exe clock %HOUR% %MIN%
goto end
:again
echo Narrowing the candidates from the full scan with %HOUR%:%MIN%...
vi-rpc.exe clock --again %HOUR% %MIN%
:end
echo.
pause
