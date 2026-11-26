@echo off
rem Confirms how the camera block encodes the world position: two snapshots around a walk due NORTH.
rem   1. stand still, facing north on the minimap, press a key   -> snapshot north1
rem   2. walk straight north about 100 m, stop, press a key       -> snapshot north2
cd /d D:\Portfolio\gta6-rpc\target-scan\release
echo Stand still, facing north, then press a key.
pause >nul
vi-rpc.exe snap north1
echo Walk straight north about 100 m (use the minimap), stop, then press a key.
pause >nul
vi-rpc.exe snap north2
python -I D:\Portfolio\gta6-rpc\re\matrix.py north1 north2
pause
