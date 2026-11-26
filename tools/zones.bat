@echo off
rem Looks through the whole game memory for the area names (Port Gelhorn, Vice City, ...) and what points at them:
rem the first step to "near <place>". About 10-15 minutes; keep the game running (no need to pause).
cd /d "%~dp0..	argetelease"
vi-rpc.exe scan GELHORN Gelhorn VICE_CITY "Vice City" LEONIDA_KEYS GRASSRIVERS Grassrivers AMBROSIA Ambrosia KALAGA Kalaga > zones.txt 2>&1
type zones.txt | more
echo saved zones.txt
pause
