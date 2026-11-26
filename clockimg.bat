@echo off
rem Clock scan over the game's own image only (its globals), instant. Double-click, enter the 24-hour time the game
rem shows (5:10 pm = 17 10). First run: full image scan; later runs: narrow the survivors. Pause the game each time.
cd /d D:\Portfolio\gta6-rpc\target-scan\release
set /p HOUR=In-game hour, 24h, e.g. 17:
set /p MIN=In-game minute, e.g. 10:
if exist img-clock.txt goto again
echo Scanning the game image for %HOUR%:%MIN%...
vi-rpc.exe clock --image %HOUR% %MIN%
goto end
:again
echo Narrowing the image candidates with %HOUR%:%MIN%...
vi-rpc.exe clock --image --again %HOUR% %MIN%
:end
echo.
pause
