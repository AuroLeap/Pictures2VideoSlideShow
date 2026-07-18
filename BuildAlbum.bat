
echo %~dp0

@echo off
goto check_Permissions
::start "" /high pwsh -executionpolicy remotesigned -File  .\BuildAlbum.ps1
:Run
pwsh -executionpolicy remotesigned -File  %~dp0\BuildAlbum.ps1
goto Exit


:check_Permissions
    net session >nul 2>&1
    if %errorLevel% == 0 (
        echo "Success: Administrative permissions confirmed."
    ) else (
        echo "Warning: Script not run as Administrator, conversion will perform significantly slower as windows firewall will scan each frame file.  It is recommended to terminate this batch file (Ctrl + C) and right click and run this batch file as an administrator for optimal performance."
        pause >nul
    )

goto Run

:Exit