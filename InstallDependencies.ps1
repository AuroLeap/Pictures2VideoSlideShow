$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole] "Administrator")
if (!$isAdmin) {
    Write-Host "Please run this script as administrator."
    exit
}

if (Get-Command ffmpeg -ErrorAction SilentlyContinue) {
    Write-Host "ffmpeg is already installed."}
else{
    Write-Host "Downloading ffmpeg..."
    Invoke-WebRequest -Uri "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" -OutFile "ffmpeg.zip"

    Write-Host "Extracting..."
    # Use the native Windows zip command
    Expand-Archive -Path "ffmpeg.zip" -DestinationPath "C:\"

    $ffmpegFolder = Get-ChildItem -Path "C:\" -Filter "ffmpeg-*" -Directory
    Rename-Item -Path $ffmpegFolder.FullName -NewName "ffmpeg"

    Write-Host "Adding ffmpeg to PATH..."
    $envPath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
    [Environment]::SetEnvironmentVariable("PATH", $envPath + ";C:\ffmpeg\bin", "Machine")
    Remove-Item "ffmpeg.zip"
}

if (Get-Command HandBrakeCLI -ErrorAction SilentlyContinue) {
    Write-Host "HandBrakeCLI is already installed."}
else{
    Write-Host "Downloading HandBrakeCLI..."
    Invoke-WebRequest -Uri "https://github.com/HandBrake/HandBrake/releases/download/1.9.0/HandBrakeCLI-1.9.0-win-x86_64.zip" -OutFile "handbrake.zip"

    Write-Host "Extracting..."
    # Use the native Windows zip command
    Expand-Archive -Path "handbrake.zip" -DestinationPath "C:\handbrake\"

    #$handbrakeFolder = Get-ChildItem -Path "C:\" -Filter "handbrake-*" -Directory
    #Rename-Item -Path $handbrakeFolder.FullName -NewName "handbrake"

    Write-Host "Adding handbrake to PATH..."
    $envPath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
    [Environment]::SetEnvironmentVariable("PATH", $envPath + ";C:\handbrake", "Machine")
    Remove-Item "handbrake.zip"
}

if (Get-Command jpegr -ErrorAction SilentlyContinue) {
    Write-Host "jpegr is already installed and availalbe on the path."}
else
{
    $jpegrpath = "C:\Program Files\JPEG Lossless Rotator"
    if (Test-Path -Path $jpegrpath)
    {
        Write-Host "jpegr is installed"
    }
    else
    {
        Write-Host "Downloading JPEGR..."
        Invoke-WebRequest -Uri "https://annystudio.com/jpegr_installer.exe" -OutFile "JPEGR_Install.exe"

        Write-Host "Follow on screen instructions, press a key when installatoin is complete..."
        # Use the native Windows zip command
        Invoke-Expression "./JPEGR_Install.exe"
        Write-Host -NoNewLine 'Press any key to continue...';
        $null = $Host.UI.RawUI.ReadKey('NoEcho,IncludeKeyDown');
    }
    Write-Host "Adding jpegr to PATH..."
    $envPath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
    [Environment]::SetEnvironmentVariable("PATH", $envPath + ";"+ $jpegrpath, "Machine")
    Remove-Item "JPEGR_Install.exe"
}

if (Get-Command magick -ErrorAction SilentlyContinue) {
    Write-Host "Image Magick is already installed and availalbe on the path."}
else
{
    Write-Host "Downloading Image Magick..."
    Invoke-WebRequest -Uri "https://imagemagick.org/archive/binaries/ImageMagick-7.1.1-43-Q16-HDRI-x64-dll.exe" -OutFile "Magick.exe"

    Write-Host "Follow on screen instructions, press a key when installatoin is complete..."
    # Use the native Windows zip command
    Invoke-Expression "./Magick.exe"
    Write-Host -NoNewLine 'Press any key to continue...';
    $null = $Host.UI.RawUI.ReadKey('NoEcho,IncludeKeyDown');
    Remove-Item "Magick.exe"
}