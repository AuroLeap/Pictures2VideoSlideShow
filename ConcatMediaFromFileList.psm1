function Join-VideosFromList
{
    param (
        $FileListPathOrCSV,
        [decimal]$crossfadedur = 0.5,
        [string]$outputFile = "output.mp4",
        $vidqty = [Int] 20
    )
    $vidbr = [int]5000000
    $vidbuff = $vidbr*3
    $FileChk = Test-Path $FileListPathOrCSV -PathType Leaf
    $FldrChk = Test-Path $FileListPathOrCSV -PathType Container
    if((($FileChk.Count -eq 1) -and $FileChk -and $FileListPathOrCSV.endsWith("csv")) )
    {
        $FileList = Import-Csv -LiteralPath $FileListPathOrCSV
    }
    elseif(($FldrChk.Count -eq 1) -and $FldrChk)
    {
        $AllInputFiles = @(Get-ChildItem -LiteralPath $FileListPathOrCSV -Filter "*.mp4" -Recurse)
        $FileList = ($AllInputFiles | Select-Object -ExpandProperty FullName)
    }
    elseif(($FileListPathOrCSV.Count -gt 1) -and ($FileListPathOrCSV[0] -is [string]))
    {
        $FileList = $FileListPathOrCSV
    }
    else
    {
        try
        {
            $FileList = ($FileListPathOrCSV | Select-Object -ExpandProperty FullName)
        }
        catch
        {
            throw "Input must be a list of strings, list of files, a directory, or path to a csv."
        }
    }
    $vscript = $outputFile+"vstream.ps1"
    $ascript = $outputFile+"astream.ps1"
    $vidfile = $outputFile+"vstream.mp4"
    $audfile = $outputFile+"astream.aac"
    $finfile = $outputFile+".mp4"
    if (Test-Path -Path $outputFile){}
    else {$null = New-Item -ItemType File -Path $outputFile -Force}
    if($FileList.Count -lt 1)
    {
        throw "No files found, or format not supported"
    }
    elseif($FileList.Count -eq 1)
    {
        Write-Host "Only one file found, copying video to output"
    }
    elseif($FileList.Count -gt 1)
    {
        Write-Host "Getting properties of all video files and building full command for $outputFile..."
        $InstanceInd = [Int] 0
        $SelInd      = [Int] 0
        $VidPathVInputStr = [String[]]::new($FileList.Count)
        $VidPathAInputStr = [String[]]::new($FileList.Count)
        $VChanInputStr   = [String[]]::new($FileList.Count)
        $AChanInputStr   = [String[]]::new($FileList.Count)
        $VFadeInputStr   = [String[]]::new($FileList.Count)
        $AFadeInputStr   = [String[]]::new($FileList.Count)
        $PrevVFadeStr = "[0]"
        $PrevAFadeStr = "[0:a]"
        $NextVidOffset = 0
        $LastFile = [Int]0
        foreach ($entry in $FileList)
        {
            #Get video definition.
            $ToRun = "ffprobe -v error -select_streams v -show_entries stream=width,height,duration -of csv=p=0 `"" +$entry+ "`""
            $VPrams = Invoke-Expression $ToRun
            $splitString = $VPrams -split ","
            $Width = [Int] $splitString[0]
            $Height = [Int] $splitString[1]
            $Dur = [decimal] $splitString[2]
            $NextVidOffset = $NextVidOffset + $Dur - $crossfadedur

            ##Incriment index for use in other string definitions
            $InstanceInd ++
            $SelInd = $InstanceInd-1
            #Get video path input definition.
            $VidPathVInputStr[$SelInd] = "-an -i `""+$entry.ToString() +"`""
            $VidPathAInputStr[$SelInd] = "-vn -i `""+$entry.ToString() +"`""
            #Get video fade definitions.
            $CurrVFadeStr = "[vfade"+$InstanceInd.ToString()+"]"
            $CurrAFadeStr = "[afade"+$InstanceInd.ToString()+"]"
            if ($InstanceInd -eq ($FileList.Count-1))
            {
                $VLastAppend = ""
                $ALastAppend = ""
            }
            elseif ($InstanceInd -ge $FileList.Count)
            {
                $LastFile = 1
            }
            else
            {
                $VLastAppend = $CurrVFadeStr
                $ALastAppend = $CurrAFadeStr+";"
                #Temp override for testing:
            }
            #Temp override for testing:
            $VStreamStrInput = "["+$InstanceInd.ToString()+":v]"
            if (-not $LastFile)
            {
                $VFadeInputStr[$SelInd] = $PrevVFadeStr+$VStreamStrInput+"xfade=transition=fade:duration="+$crossfadedur.ToString()+":offset="+$NextVidOffset.ToString()+$VLastAppend+";"
            }
            $PrevVFadeStr = $CurrVFadeStr #For next iteration
            #Get audio fade definitoins.
            $AStreamStrInput = "["+$InstanceInd.ToString()+":a]"
            $AChanInputStr[$SelInd] = "["+ $SelInd.ToString()+":a]" + "asettb=AVTB" +$PrevAFadeStr +"`;"
            if (-not $LastFile)
            {
                $AFadeInputStr[$SelInd] = $PrevAFadeStr+$AStreamStrInput+"acrossfade=d="+$crossfadedur.ToString()+$ALastAppend
            }
            $PrevAFadeStr = $CurrAFadeStr #For next iteration

        }
        #Write-Host "Building final command string"
        $CmdPartVInput = $VidPathVInputStr -join " \`n"
        $CmdPartAInput = $VidPathAInputStr -join " \`n"
        $CmdPartVFade = $VFadeInputStr -join "\`n"
        $CmdPartAChan = $AChanInputStr -join "\`n"
        $CmdPartAFade = $AFadeInputStr -join "\`n"
        $VCmdPartEnded = " -vcodec libx265 -crf $vidqty -preset slow -pix_fmt yuv420p -movflags faststart `"$vidfile`""
        $ACmdPartEnded = " -acodec aac -movflags faststart `"$audfile`""
# -maxrate=$vidbr -bufsize $vidbuff
        #$FullCmdStart = "ffmpeg -y "+$CmdPartInput
        $FilterDef = "-filter_complex \`n`""
        $VFiltChain = $FilterDef+$CmdPartVFade + "`"\`n"
        $AFiltChain = $FilterDef+$CmdPartAFade + "`"\`n"
        $VPreCmd = "ffmpeg -y " +  "\`n" + $CmdPartVInput + " " + $VFiltChain + " " + $VCmdPartEnded
        $APreCmd = "ffmpeg -y " +  "\`n" + $CmdPartAInput + " " + $AFiltChain + " " + $ACmdPartEnded
        #$PreCmd = $FullCmdStart + "\`n" + $CmdPartVChan + "\`n" + $CmdPartVFade + "\`n" + $CmdPartAChan + "\`n" + $CmdPartAFade + "`"\`n" + $CmdPartEnded
        #$PreCmd = $FullCmdStart + "\`n" + $CmdPartVChan + "\`n" + $CmdPartVFade + "\`n" + $CmdPartAFade + "`"\`n" + $CmdPartEnded
        #$PreCmd = $FullCmdStart + "\`n" + $CmdPartVChan + "\`n" + $CmdPartVFade + "`"\`n" +  $CmdPartEnded
        $FullVCmd = $VPreCmd -replace '\\\r?\n',''
        $FullACmd = $APreCmd -replace '\\\r?\n',''
        if($FullVCmd.Length -gt 32767)
        {
            throw "Video contruct command larger than maximum allowed limit.  Reduce the number of file inputs to reduce concat limit."
        }
        if($FullACmd.Length -gt 32767)
        {
            throw "Audio contruct command larger than maximum allowed limit.  Reduce the number of file inputs to reduce concat limit."
        }
        #Build scripts to run.
        $FullVCmd | Out-File -FilePath $vscript -force
        $FullACmd | Out-File -FilePath $ascript -force
        Write-Host "Building video for $outputFile..."
        Invoke-Expression $vscript
        Invoke-Expression $ascript
        $FullCmd = "ffmpeg -y -an -i `"$vidfile`" -vn -i `"$audfile`" -c copy `"$finfile`""
        Invoke-Expression $FullCmd
        #(Invoke-Expression $FullCmd) *> $null
        Write-Host "$finfile complete"
    }
}