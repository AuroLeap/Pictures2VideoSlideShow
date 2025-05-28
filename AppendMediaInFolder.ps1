function Append-MediaWithFade {
    param (
        [string]$inputFolder,
        [string]$outputFile = "output.mp4"
    )

    # Create a temporary directory to store the intermediate files
    $tempDir = Join-Path $env:TEMP "ffmpeg_concat"
    if (Test-Path $tempDir) {
        Remove-Item -Recurse -Force $tempDir
    }
    New-Item -ItemType Directory -Path $tempDir

    # Get all video and image files in the folder
    $mediaFiles = Get-ChildItem -Path $inputFolder -File | Where-Object { $_.Extension -match '\.(jpg|jpeg|png|mp4|mov|avi)' }

    # Create a list to hold the input files and their transitions
    $fileList = @()

    foreach ($mediaFile in $mediaFiles) {
        $fileName = [System.IO.Path]::GetFileNameWithoutExtension($mediaFile.Name)
        $extension = $mediaFile.Extension

        if ($extension -match 'jpg|jpeg|png') {
            # Convert image to video with a duration of 5 seconds (you can adjust as needed)
            $imageVideoPath = Join-Path $tempDir "$fileName.mp4"
            ffmpeg -loop 1 -framerate 1 -t 5 -i $mediaFile.FullName -c:v libx264 -pix_fmt yuv420p -r 30 -y $imageVideoPath
            $fileList += $imageVideoPath
        } else {
            $fileList += $mediaFile.FullName
        }
    }

    # Create a file list for FFmpeg concatenation
    $concatListPath = Join-Path $tempDir "concat_list.txt"
    $fileList | ForEach-Object { "file '$($_)'" } | Out-File $concatListPath -Encoding UTF8

    # Concatenate the files with 1-second fade transition
    $fadeDuration = 1
    $outputFilePath = Join-Path $tempDir $outputFile

    ffmpeg -f concat -safe 0 -i $concatListPath -filter_complex "
        [0:v]fade=t=out:st=4:d=$fadeDuration[v1];
        [1:v]fade=t=in:st=0:d=$fadeDuration[v2];
        [v1][v2]concat=n=2:v=1:a=0[out]" -map "[out]" -y $outputFilePath

    # Move the final output to the desired location
    Move-Item -Path $outputFilePath -Destination $outputFile

    # Clean up temporary files
    Remove-Item -Recurse -Force $tempDir
    Write-Host "Final video saved as $outputFile"
}

# Example usage:
# Append-MediaWithFade -inputFolder "C:\path\to\media" -outputFile "C:\path\to\output\final_video.mp4"
