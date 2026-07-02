@echo off
setlocal
REM Product launcher (Windows) — double-click to run this project.
REM Every launchable project ships run.cmd / run.sh / run.command (process.md
REM section 7, "the evaluator's rungs") so starting it never requires recalling
REM a command. Read it first; it only runs the one command below.
REM
REM Not applicable (a pure library)? Delete the run.* launchers and describe
REM usage in README.md instead.

REM --- EDIT FOR YOUR PROJECT ---------------------------------------------------
REM Dev-checkout launcher: builds (cached after the first time) and starts the
REM engine. With no config the exe opens the first-run GUI wizard; pass args
REM through, e.g.:  run.cmd --config your_config.toml build
REM End users don't need this file — they download make_video_slideshow.exe
REM (docs/quick-reference.md section 1). Windows-only product: the POSIX
REM run.sh/run.command twins are deliberately not shipped.
set "RUN_CMD=cargo run --release --"
REM ----------------------------------------------------------------------------

cd /d "%~dp0"
if not defined RUN_CMD (
  echo run.cmd: no launch command wired yet.
  echo Edit RUN_CMD in this file — see the EDIT FOR YOUR PROJECT block — and
  echo in run.sh. The README "Run it" section documents the underlying command.
  pause
  exit /b 1
)
echo Running: %RUN_CMD% %*
%RUN_CMD% %*
set "EXITCODE=%ERRORLEVEL%"
echo.
echo Exited with code %EXITCODE%.
pause
exit /b %EXITCODE%
