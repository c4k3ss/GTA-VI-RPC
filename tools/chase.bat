@echo off
rem Finds the game's fixed path to the clock places left by clock.bat (two hops at most). A few minutes; pause the game.
cd /d "%~dp0..\target\release"
vi-rpc.exe chase
pause
