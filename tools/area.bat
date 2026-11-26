@echo off
rem Finds the field the game updates when the map's STREET (or area) label changes. Three snapshots, map closed:
rem   1. on street A, press a key                         -> snapshot areaA
rem   2. further along the SAME street, press a key       -> snapshot areaA2 (filters fields that just drift)
rem   3. after walking onto ANOTHER street, press a key   -> snapshot areaB
cd /d "%~dp0..	argetelease"
echo On the first street, map closed, press a key.
pause >nul
vi-rpc.exe snap areaA
echo Move further along the SAME street, then press a key.
pause >nul
vi-rpc.exe snap areaA2
echo Walk onto ANOTHER street (check the map, then close it), then press a key.
pause >nul
vi-rpc.exe snap areaB
vi-rpc.exe udiff areaA areaA2 areaB
pause
