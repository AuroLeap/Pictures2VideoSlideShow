# Set the folder path where your videos are stored.
# Modify this to your target folder.
$FolderPath = "Z:\Album1920x1080_FPS30_DT6_FT0.7_MR20_q17\"

# Define an array of common video file extensions.
$videoExtensions = @("*.mp4", "*.mov", "*.avi", "*.mkv", "*.wmv", "*.flv", "*.ts")

# Process each file type one by one.
foreach ($ext in $videoExtensions) {
    # Get all files matching the current extension recursively.
    $files = Get-ChildItem -Path $FolderPath -Filter $ext -File -Recurse -ErrorAction SilentlyContinue
    foreach ($file in $files) {
        try {
            # Build the ffprobe command.
            # This command extracts the width of the first video stream.
            $ffprobeArgs = "-v error -show_streams -select_streams v:0 -show_entries stream=width -of csv=p=0 `"$($file.FullName)`""
            #ffprobe -v error -show_streams -select_streams v`:0 -of ini
            # Execute ffprobe and capture the output.
            $widthOutput = ffprobe $ffprobeArgs 2>&1 | Out-String
            $widthOutput = $widthOutput.Trim()
            Write-Output  $widthOutput
            # Validate that we got a number and if it equals 1080 pixels.
            if ([int]::TryParse($widthOutput, [ref]$null) -and [int]$widthOutput -eq 1080) {
                Write-Output "Deleting file: $($file.FullName) (Width: $widthOutput)"
                # Remove the file. Remove -WhatIf after testing if everything is correct.
                Remove-Item $file.FullName -Force #-WhatIf
            }
        }
        catch {
            Write-Warning "Error processing file $($file.FullName): $_"
        }
    }
}
