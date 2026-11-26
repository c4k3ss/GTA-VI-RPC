@echo off
rem Shows the bytes around the clock places left by clock.bat.
cd /d "%~dp0..\target\release"
elease"
elease"
vi-rpc.exe peek %* > peek.txt
type peek.txt
echo saved peek.txt
pause
