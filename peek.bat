@echo off
rem Shows the bytes around the clock places left by clock.bat.
cd /d D:/Portfolio/gta6-rpc/target-scan/release
vi-rpc.exe peek %* > peek.txt
type peek.txt
echo saved peek.txt
pause
