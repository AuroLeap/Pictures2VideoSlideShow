@echo off
setlocal EnableDelayedExpansion
rem ===========================================================================
rem  Pictures2VideoSlideShow - one-click demo
rem
rem  Downloads a few free-use sample photos & video clips (Wikimedia Commons,
rem  public-domain / CC0) into a "demo-media" folder beside this script, then
rem  runs make_video_slideshow.exe with the bundled demo_config.toml + focus.json
rem  to produce a slideshow MP4 in "demo-output".
rem
rem  Just double-click this file. It expects make_video_slideshow.exe to sit in
rem  the same folder (it does in the extracted release zip).
rem
rem  Uses only built-in Windows tools (curl.exe + certutil). Re-running is cheap:
rem  already-downloaded media is reused and only re-verified.
rem ===========================================================================

rem Operate in this script's own folder so all relative paths resolve here.
cd /d "%~dp0"

set "EXE=make_video_slideshow.exe"
set "CONFIG=demo_config.toml"
set "MEDIA=demo-media"
set "OUTPUT=demo-output"

echo ============================================================
echo  Pictures2VideoSlideShow - demo
echo ============================================================
echo.

rem --- 1. The exe must be right here (it is in the release zip) ---------------
if not exist "%EXE%" (
  echo ERROR: "%EXE%" was not found next to this script.
  echo.
  echo   This demo expects the binary in the SAME folder as demo.bat. Either:
  echo     - extract the whole release .zip and run demo.bat from that folder, or
  echo     - build it yourself with:  cargo build --release
  echo       then copy target\release\%EXE% next to this script.
  echo.
  pause
  exit /b 1
)

rem --- 2. curl.exe is required (built into Windows 10 1803+ / Windows 11) -----
where curl.exe >nul 2>&1
if errorlevel 1 (
  echo ERROR: curl.exe was not found on this system.
  echo   curl ships with Windows 10 1803 and later. Please update Windows, or
  echo   download the sample media manually - see README.md for the URLs.
  echo.
  pause
  exit /b 1
)

if not exist "%MEDIA%" mkdir "%MEDIA%"

rem --- 3. Download + checksum-verify each sample file ------------------------
rem  call :fetch <local-name> <url> <expected-sha256>
echo Fetching sample media into "%MEDIA%"\ ...
echo.

call :fetch "cat.jpg"       "https://upload.wikimedia.org/wikipedia/commons/c/c7/Tabby_cat_with_blue_eyes-3336579.jpg" "f91f1e37a23344251f40a7731b28b6612fef6cc37a2f1e7f97d5f6610063248c"
if errorlevel 1 goto :fail
call :fetch "flower.jpg"    "https://upload.wikimedia.org/wikipedia/commons/3/34/Chaenomeles_japonica_CC0_1.0.jpg" "07148b977b236f1f684ea642cee5f2f0c4bb2aa5423d0dd467d7ad6f36d82c20"
if errorlevel 1 goto :fail
call :fetch "sunflower.jpg" "https://upload.wikimedia.org/wikipedia/commons/7/77/Red_sunflower.jpg" "f83b4ed63a370d4d31e8e39e70ff16c7681a3f69e3d76b796aa9ba7c64fb6c60"
if errorlevel 1 goto :fail
call :fetch "clouds1.webm"  "https://upload.wikimedia.org/wikipedia/commons/e/e2/Wave_clouds_on_the_lee_of_the_Rocky_Mountains_(CIRA_2019-11-19).webm" "0b77f661fc7d78bd2c97b7dc24e8181dcbc96b7ce2ac50bc44380c674fb0c328"
if errorlevel 1 goto :fail
call :fetch "clouds2.webm"  "https://upload.wikimedia.org/wikipedia/commons/f/f7/Waves_in_the_Clouds_off_the_Northern_California_Coast_(CIRA_2025-07-15_-_nolabels).webm" "93bd93a4df605ad2a6db5145c9102997a43ebbd602daf596ff3479182f82e500"
if errorlevel 1 goto :fail

echo.
echo All sample media present and verified.
echo.

rem --- 4. Validate the runtime prerequisites before the long build ------------
rem  (Interactive, so a missing FFmpeg can be auto-fetched on a clean machine.)
echo ------------------------------------------------------------
echo  Step 1/2: validate
echo ------------------------------------------------------------
"%EXE%" --config "%CONFIG%" validate
if errorlevel 1 (
  echo.
  echo ERROR: validate failed - see the messages above. Build skipped.
  echo.
  pause
  exit /b 1
)

rem --- 5. Build the slideshow ------------------------------------------------
echo.
echo ------------------------------------------------------------
echo  Step 2/2: build
echo ------------------------------------------------------------
"%EXE%" --config "%CONFIG%" build
if errorlevel 1 (
  echo.
  echo ERROR: build failed - see the messages above.
  echo.
  pause
  exit /b 1
)

echo.
echo ============================================================
echo  Done! Your demo slideshow is in:  %CD%\%OUTPUT%
echo ============================================================
echo.
pause
exit /b 0

rem ---------------------------------------------------------------------------
rem  :fetch <local-name> <url> <expected-lowercase-sha256>
rem  NOTE: uses delayed expansion (!var!) and keeps the download on a flat line
rem  so a URL containing literal "(" ")" cannot break batch block parsing, and
rem  passes URLs WITHOUT percent-escapes so `call` does not double-expand them.
:fetch
set "name=%~1"
set "url=%~2"
set "want=%~3"

if exist "!MEDIA!\!name!" ( echo   !name! - already downloaded, re-verifying... & goto :fetch_verify )
echo   !name! - downloading...
curl -L --fail --silent --show-error -o "!MEDIA!\!name!" "!url!"
if errorlevel 1 ( echo   ERROR: download failed for !name! & exit /b 1 )

:fetch_verify
rem  Compute SHA-256 with certutil; the hash is the 2nd line of its output.
set "got="
for /f "skip=1 delims=" %%H in ('certutil -hashfile "!MEDIA!\!name!" SHA256') do if not defined got set "got=%%H"
rem  Strip any spaces older certutil builds insert between hex bytes.
set "got=!got: =!"

if /i not "!got!"=="!want!" (
  echo   ERROR: checksum mismatch for !name! - the download may be corrupt or tampered.
  echo          expected: !want!
  echo          got:      !got!
  del /q "!MEDIA!\!name!" >nul 2>&1
  exit /b 1
)
echo   !name! - OK ^(sha256 verified^)
exit /b 0

:fail
echo.
echo Demo aborted: could not obtain the sample media. Check your internet
echo connection and try again. (See README.md for the direct download URLs.)
echo.
pause
exit /b 1
