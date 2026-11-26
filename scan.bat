@echo off
rem Read-only scan of the running game for scene-name strings and pointers at them (about 5-15 min). Run in the world.
cd /d D:\Portfolio\gta6-rpc\target-scan\release
vi-rpc.exe scan %*
pause
