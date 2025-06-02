$VideoIn = "D:\SlowMo.mp4"
$OutDir = "D:\OutVid\"
if (Test-Path -LiteralPath $OutDir)
{
    New-Item -Path $OutDir -ItemType "directory" | Out-Null
}
ffmpeg -i input.mp4 -c:v png ($OutDir + "output_frame%04d.png")