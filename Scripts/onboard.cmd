@echo off
setlocal enabledelayedexpansion
REM Stage-0 onboarder (Windows) — the first rung of the onboarding ladder
REM (process.md section 7):
REM
REM     Stage 0  ->  dev-setup  ->  setup  ->  check
REM     (this)       workstation     deps       gates
REM
REM Takes someone from a bare machine to an editable, viewable checkout: it
REM ensures git, shows a folder picker, clones over HTTPS, then hands off to
REM dev-setup.ps1. You do NOT need to know git to use it — an AI agent can drive
REM git for you afterward (see the end banner).
REM
REM The .cmd extension makes this double-clickable in Explorer. Read it first
REM (it is meant to be read). It installs nothing without asking and never pipes
REM code from the internet into a shell. Windows SmartScreen may warn about an
REM unsigned script — that is expected for a file you downloaded and read.
REM
REM Linux contributors: use onboard.sh.  macOS: use onboard.command.

REM --- EDIT FOR YOUR PROJECT ---------------------------------------------------
REM bootstrap.py copies this file as-is; fill in your repo's HTTPS clone URL.
REM The project may then attach onboard.cmd to a GitHub Release.
set "REPO_URL=https://github.com/OWNER/REPO.git"
REM ----------------------------------------------------------------------------

echo ------------------------------------------------------------------
echo This will set up a working copy of:
echo     %REPO_URL%
echo.
echo Steps (each shown before it runs):
echo   1. make sure 'git' is installed
echo   2. show a folder picker to choose where the project goes
echo   3. download (clone) the project into that folder over HTTPS
echo   4. offer to check your developer workstation (dev-setup)
echo.
echo It installs nothing without asking first.
echo ------------------------------------------------------------------
set /p "reply=Continue? [y/N] "
if /i not "!reply!"=="y" (
  echo Cancelled — nothing was changed.
  goto :end
)

REM 2. Ensure git is available (offer winget, then choco).
where git >nul 2>nul
if errorlevel 1 (
  echo.
  echo 'git' is not installed.
  where winget >nul 2>nul
  if not errorlevel 1 (
    set /p "ok=Run "winget install --id Git.Git -e" now? [y/N] "
    if /i "!ok!"=="y" ( winget install --id Git.Git -e --source winget ) else ( goto :needgit )
  ) else (
    where choco >nul 2>nul
    if not errorlevel 1 (
      set /p "ok=Run "choco install git -y" now? [y/N] "
      if /i "!ok!"=="y" ( choco install git -y ) else ( goto :needgit )
    ) else (
      goto :needgit
    )
  )
  echo Re-open this window (so git is on PATH), then run onboard.cmd again.
  goto :end
)

REM 3. Native folder picker via PowerShell's FolderBrowserDialog.
set "DEST_PARENT="
for /f "usebackq delims=" %%D in (`powershell -NoProfile -STA -Command "Add-Type -AssemblyName System.Windows.Forms | Out-Null; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = 'Pick a folder to put the project in'; if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }"`) do set "DEST_PARENT=%%D"
if not defined DEST_PARENT (
  set /p "DEST_PARENT=No folder picked. Type a path [%USERPROFILE%]: "
  if not defined DEST_PARENT set "DEST_PARENT=%USERPROFILE%"
)

REM Derive "<parent>\<repo-name>" from the URL's last path segment.
for /f "usebackq delims=" %%N in (`powershell -NoProfile -Command "[System.IO.Path]::GetFileNameWithoutExtension('%REPO_URL%')"`) do set "NAME=%%N"
set "DEST=!DEST_PARENT!\!NAME!"

REM 4. Clone over HTTPS. (Private repo or pushing later: authenticate with the
REM    host CLI — e.g. `gh auth login` — not hand-rolled SSH keys.)
echo.
echo Cloning into: !DEST!
if exist "!DEST!" (
  echo That folder already exists — using it as-is (skipping clone).
) else (
  git clone "%REPO_URL%" "!DEST!"
)

REM 5. End banner: checkout dir + agent handoff, then offer dev-setup.
echo.
echo ------------------------------------------------------------------
echo   Your checkout is ready at:
echo       !DEST!
echo.
echo   If you'd like an AI agent to manage your changes (commits, pushes,
echo   reviews) for you, point it at this directory.
echo ------------------------------------------------------------------
echo.

set "DEV_SETUP=!DEST!\scripts\dev-setup.ps1"
if exist "!DEV_SETUP!" (
  set /p "go=Check your developer workstation now (dev-setup -Check, installs nothing)? [Y/n] "
  if /i "!go!"=="n" (
    echo Skipped. Run it later with: powershell -ExecutionPolicy Bypass -File "!DEV_SETUP!" -Check
  ) else (
    powershell -NoProfile -ExecutionPolicy Bypass -File "!DEV_SETUP!" -Check
    echo.
    echo To install the baseline workstation: powershell -ExecutionPolicy Bypass -File "!DEV_SETUP!" -Baseline
  )
) else (
  echo Next: cd /d "!DEST!" and run  powershell -ExecutionPolicy Bypass -File scripts\dev-setup.ps1 -Check
)
goto :end

:needgit
echo Install git from https://git-scm.com/download/win , then re-run onboard.cmd.

:end
echo.
pause
endlocal
