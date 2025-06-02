# Define the folder path containing images and the output video file
$folderPath = "D:\fps_test"
$outputVideo = "D:\ToDo\TestVideo.mp4"
$outputVideo2 = "D:\ToDo\TestVideoNumbered.mp4"

# Create a temp folder to store the numbered images
$tempFolder = "$folderPath\temp_images"
if (Test-Path $tempFolder) {
    Remove-Item -Recurse -Force $tempFolder
}
New-Item -Path $tempFolder -ItemType Directory

# Get all image files in the folder (assuming they are all .jpg, you can adjust this)
$imageFiles = Get-ChildItem -Path $folderPath -Filter "*.jpg" | Sort-Object Name

# Loop through the images and rename them to be numbered
$counter = 1
foreach ($image in $imageFiles) {
    $newImageName = "{0:D4}.jpg" -f $counter  # This will give the format 0001.jpg, 0002.jpg, etc.
    $newImagePath = Join-Path -Path $tempFolder -ChildPath $newImageName
    Copy-Item -Path $image.FullName -Destination $newImagePath
    $counter++
}

# Use FFmpeg to create a video from the numbered images
# Adjust the frame rate (e.g., 30 frames per second)
$frameRate = 30
$ffmpegCmd = "ffmpeg -framerate $frameRate -i `"$tempFolder\%04d.jpg`" -c:v libx264 -pix_fmt yuv420p -r $frameRate `"$outputVideo`""

# Execute the FFmpeg command
Invoke-Expression $ffmpegCmd


$ffmpegCmd2 = "ffmpeg -i $outputVideo -vf `"drawtext=fontfile=Arial.ttf: text='%{frame_num}': start_number=1: x=(w-tw)/2: y=h-(2*lh): fontcolor=black: fontsize=200: box=1: boxcolor=white: boxborderw=5`" -c:a copy $outputVideo2"

Invoke-Expression $ffmpegCmd2

# Clean up the temporary images folder
Remove-Item -Recurse -Force $tempFolder

Write-Host "Video has been created successfully at: $outputVideo"
