@echo off
rem Confirms how the camera block encodes the world position: two snapshots around a walk due NORTH.
rem   1. stand still, facing north on the minimap, press a key   -> snapshot north1
rem   2. walk straight north about 100 m, stop, press a key       -> snapshot north2
cd /d "%~dp0..\target\release"
elease"
echo Stand still, facing north, then press a key.
pause >nul
vi-rpc.exe snap north1
echo Walk straight north about 100 m (use the minimap), stop, then press a key.
pause >nul
vi-rpc.exe snap north2
python -I "%~dp0..\re\matrix.py" north1 north2
pause
