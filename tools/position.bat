@echo off
rem Finds where the game keeps the player's position, by snapshots of the game image (instant each):
rem   1. stand still in the world, press a key here            -> snapshot "still1"
rem   2. keep standing still ~10 s, press a key                 -> snapshot "still2"
rem   3. walk or drive a good distance (50+ m), stop, press a key -> snapshot "moved"
rem Then it prints the float triples that held still and then moved like a position.
cd /d "%~dp0..	argetelease"
echo Stand still in the world, then press a key.
pause >nul
vi-rpc.exe snap still1
echo Keep standing still for about 10 seconds, then press a key.
pause >nul
vi-rpc.exe snap still2
echo Now move a good distance, stop, then press a key.
pause >nul
vi-rpc.exe snap moved
vi-rpc.exe diff still1 still2 moved
pause
