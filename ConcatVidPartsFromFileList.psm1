function Join-VidPartsFromList
{
    param (
        $GenFrmt,
        $FileListProps,
        [string]$outputFile = "output",
        $vidqty = [Int] 20,
        $IncAud = [Int] 1,
        $SelFPS = [Decimal] 24
    )
    if(($FileListProps.Count -gt 1) -and ($FileListProps[0] -is [string]))
    {
        $FileList = $FileListProps
    }
    else{
        throw "Input must be a list of strings."
    }
    $grpfldr = $outputFile
    $tranprepend = $grpfldr+"\t"
    $appendlist = $grpfldr+"\buildlist.txt"
    $buildcmd   = $grpfldr+"\buildcmd.txt"
    $finfile = $outputFile+".$GenFrmt"
    if (Test-Path -Path $grpfldr -PathType Container){}
    else {(New-Item -ItemType Directory -Path $grpfldr -Force) *> $null}
    try{

        if($FileList.Count -lt 1)
        {
            throw "No files found, or format not supported"
        }
        else
        {
            Write-Host "Getting properties of all video files and building full command for $outputFile..."

            $FileList | Add-Member -MemberType NoteProperty -Name VidDef -Value $([System.ValueTuple[int, int, int, decimal,string, string]])
            $FileList | Add-Member -MemberType NoteProperty -Name Dur -Value $([decimal])
            $FileList | Add-Member -MemberType NoteProperty -Name SrtDur -Value $([decimal])
            $FileList | Add-Member -MemberType NoteProperty -Name EndDur -Value $([decimal])
            $FileList | Add-Member -MemberType NoteProperty -Name FrameRate -Value $([decimal])
            $FileList | Add-Member -MemberType NoteProperty -Name Need2Conv -Value $([int]0)
            $FileList | Add-Member -MemberType NoteProperty -Name colorspace -Value $([string]"")
            $FileList | Add-Member -MemberType NoteProperty -Name pixfmt -Value $([string]"")
            $FileList | Add-Member -MemberType NoteProperty -Name vcodec -Value $([string]"")
            $FileList | Add-Member -MemberType NoteProperty -Name acodec -Value $([string]"")
            $FileList | Add-Member -MemberType NoteProperty -Name arate -Value $([decimal]0)
            $FileList | Add-Member -MemberType NoteProperty -Name width -Value $([int]0)
            $FileList | Add-Member -MemberType NoteProperty -Name height -Value $([int]0)
            $FileList | Add-Member -MemberType NoteProperty -Name SortInd -Value $([int]0)
            $FileList | Add-Member -MemberType NoteProperty -Name timebase -Value $([int]0)
            $FileList | Add-Member -MemberType NoteProperty -Name atimebase -Value $([int]0)
            $FileList | Add-Member -MemberType NoteProperty -Name nomexppath -Value $([string]"")
            $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
            $NfilesProc = 0;
            #*********************************************************************
            #*********************** Get file properties *************************
            #*********************************************************************
            #$FileList = @($FileList[0..2])
            foreach ($entry in $FileList)
            {
                #Get video definition.
                $SrtChkName = $entry+"srt"
                $NomChkName = $entry
                $EndChkName = $entry+"end"
                $VPramsCmd = "ffprobe -v error -show_streams -select_streams v`:0 -of ini `"$NomChkName`""
                $VSrtPramsCmd = "ffprobe -v error -show_streams -select_streams v`:0 -of ini `"$SrtChkName`""
                $VEndPramsCmd = "ffprobe -v error -show_streams -select_streams v`:0 -of ini `"$EndChkName`""
                $APramsCmd = "ffprobe -v error -show_streams -select_streams a`:0 -of ini `"$NomChkName`""
                $VPrams = Invoke-Expression $VPramsCmd
                $VSrtPrams = Invoke-Expression $VSrtPramsCmd
                $VEndPrams = Invoke-Expression $VEndPramsCmd
                $APrams = Invoke-Expression $APramsCmd
                $PreWidth = [Int]::0
                $PreHeight = [Int]::0
                $timebase = 0
                $atimebase = 0
                $colorspace = ""
                $pixfmt = ""
                $vcodec = ""
                $vcodecprofile = ""
                $vcodeclevel = ""
                $refcnt = ""
                $acodec = ""
                $achnls = ""
                $afrmt = ""
                $aprofile = ""
                $alayout = ""
                $arate = 0
                $framerate = 0
                $Dur = 0; $SrtDur = 0; $EndDur = 0
                foreach ($Pram in $VSrtPrams)
                {
                    if ( $Pram.StartsWith("duration="))
                    {
                        $SrtDur = [decimal]::Parse($Pram.split('duration=')[1])
                    }
                }
                foreach ($Pram in $VEndPrams)
                {
                    if ( $Pram.StartsWith("duration="))
                    {
                        $EndDur = [decimal]::Parse($Pram.split('duration=')[1])
                    }
                }
                if ($VPrams.Count -gt 1)
                {
                    foreach ($Pram in $VPrams)
                    {
                        if ( $Pram.StartsWith("width="))
                        {
                            $PreWidth = [Int]::Parse($Pram.split('width=')[1])
                        }
                        if ( $Pram.StartsWith("height="))
                        {
                            $PreHeight = [Int]::Parse($Pram.split('height=')[1])
                        }
                        if ( $Pram.StartsWith("avg_frame_rate="))
                        {
                            $framerate = [decimal]::Parse( (Invoke-Expression ($Pram.split('avg_frame_rate=')[1])))
                        }
                        if ( $Pram.StartsWith("rotation="))
                        {
                            $Rotation = [Int]::Parse($Pram.split('rotation=')[1])
                        }
                        if ( $Pram.StartsWith("duration="))
                        {
                            $Dur = [decimal]::Parse($Pram.split('duration=')[1])
                        }
                        if ( $Pram.StartsWith("time_base="))
                        {
                            $timebase = [Int] (1/([decimal] (Invoke-Expression ($Pram.split('time_base=')[1]))))
                        }
                        if ( $Pram.StartsWith("color_space="))
                        {
                            $colorspace = $Pram.split('color_space=')[1]
                        }
                        if ( $Pram.StartsWith("pix_fmt="))
                        {
                            $pixfmt = $Pram.split('pix_fmt=')[1]
                        }
                        if ( $Pram.StartsWith("codec_name="))
                        {
                            $vcodec = $Pram.split('codec_name=')[1]
                        }
                        if ( $Pram.StartsWith("level="))
                        {
                            $vcodeclevel = $Pram.split('level=')[1]
                        }
                        if ( $Pram.StartsWith("profile="))
                        {
                            $vcodecprofile = $Pram.split('profile=')[1]
                        }
                        if ( $Pram.StartsWith("refs="))
                        {
                            $refcnt = $Pram.split('refs=')[1]
                        }
                    }
                    foreach ($Pram in $APrams)
                    {
                        if ( $Pram.StartsWith("codec_name="))
                        {
                            $acodec = $Pram.split('codec_name=')[1]
                        }
                        if ( $Pram.StartsWith("sample_rate="))
                        {
                            $arate = $Pram.split('sample_rate=')[1]
                        }
                        if ( $Pram.StartsWith("channels="))
                        {
                            $achnls = $Pram.split('channels=')[1]
                        }
                        if ( $Pram.StartsWith("sample_fmt="))
                        {
                            $afrmt = $Pram.split('sample_fmt=')[1]
                        }
                        if ( $Pram.StartsWith("profile="))
                        {
                            $aprofile = $Pram.split('profile=')[1]
                        }
                        if ( $Pram.StartsWith("channel_layout="))
                        {
                            $alayout = $Pram.split('channel_layout=')[1]
                        }
                        if ( $Pram.StartsWith("time_base="))
                        {
                            $atimebase = [Int] (1/([decimal] (Invoke-Expression ($Pram.split('time_base=')[1]))))
                        }
                    }

                }
                $Width = $PreWidth
                $Height = $PreHeight
                $vdefinition = $colorspace+$pixfmt+$vcodec+$vcodeclevel+$vcodecprofile+$refcnt
                $adefinition = $acodec+$achnls+$afrmt+$aprofile+$alayout
                # pixel format, colorspace, vcodec, acodec, Width, height, timebase, arate,
                $entry.VidDef = [System.ValueTuple[int, int, int, decimal, string, string]]::new(
                        $Width, $Height, $timebase, $arate, $vdefinition, $adefinition)

                $entry.framerate = $framerate; $entry.timebase = $timebase;
                $entry.Dur = $Dur ; $entry.SrtDur = $SrtDur ; $entry.EndDur = $EndDur
                $entry.width = $width; $entry.height = $height; $entry.vcodec = $vcodec
                $entry.acodec = $acodec; $entry.arate  = $arate; $entry.pixfmt = $pixfmt
                $entry.atimebase = $atimebase; $entry.colorspace = $colorspace
                $NfilesProc++
                $entry.SortInd = $NfilesProc
                if($stopwatch.ElapsedMilliseconds -gt 5000)
                {
                    write-host "$outputFile`: $outputFile`: Imported file $NfilesProc of $($FileList.Count)"
                    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
                }
            }
            #*********************************************************************
            #*********************** End of property import *************************
            #*********************************************************************
            $NotInFirstGrp = 0
            $GrpSets = $FileList | Group-Object -Property VidDef
            #$FileList | Add-Member -MemberType NoteProperty -Name FrameRate -Value $([decimal])
            $GrpSets | Add-Member -MemberType NoteProperty -Name TotalDur -value $([decimal]0)
            foreach ($Grp in $GrpSets)
            {
                foreach ($file in ($Grp| Select-Object -Expand Group))
                {
                    $Grp.TotalDur = $Grp.TotalDur + $file.Dur
                }
            }
            $GrpSets = $GrpSets| Sort-Object TotalDur -Descending
            foreach ($grp in $GrpSets){
                if($NotInFirstGrp)
                {
                    #($file in ($_| Select-Object -Expand Group))
                    foreach($file in ($grp| Select-Object -ExpandProperty Group))
                    {
                        $file.Need2Conv = 1
                    }


                }
                #$colorspace+$pixfmt+$vcodec
                #This is only entered for the first group, the group with the most items.
                else{
                    $expgrp = ($grp| Select-Object -ExpandProperty Group)
                    $selcolorspace = $expgrp[0].colorspace
                    $selpixfmt = $expgrp[0].pixfmt
                    $selvcodec = $expgrp[0].vcodec
                    $selacodec = $expgrp[0].acodec
                    $selarate = $expgrp[0].arate
                    $vseltimebase = $expgrp[0].timebase
                    $aseltimebase = $expgrp[0].atimebase
                }
                $NotInFirstGrp++
            }
            if ($NotInFirstGrp -gt 1)
            {
                Write-Host "$grpfldr`: Video sets for are not all the same properties, some files may be ignored, or converted."
            }

            #*********************************************************************
            #*********************** Perpare transitions  *************************
            #*********************************************************************
            #Only pick files with common format, since they must be concatable.
            $EncodeDef = "-video_track_timescale $vseltimebase -vcodec $selvcodec -crf $vidqty -preset slow -pix_fmt $selpixfmt -colorspace $selcolorspace -movflags faststart "
            $FileList = @($GrpSets |  Select-Object -ExpandProperty Group) | Where-Object -Property Need2Conv -eq 0
            $FileList | Add-Member -MemberType NoteProperty -Name ExportSuccess -Value $([int]0)
            $NFilesExported = 0
            $LastExpIdx = -1
            $CurrExpIdx = 0
            $LastVid = 0
            $PrevVid2TransitionFrom = ""
            $PrevVidDuration = 0
            $VidPathStr = [String[]]::new($FileList.Count*2+1)
            foreach ($file in $FileList)
            {
                $CurrIdx++
                try
                {
                    $postname = [System.IO.Path]::GetFileNameWithoutExtension($file)
                    $VSrt = $file+"srt"
                    $VEnd = $file+"end"
                    if($PrevVid2TransitionFrom.Length)
                    {
                        $prename  = [System.IO.Path]::GetFileNameWithoutExtension($PrevVid2TransitionFrom)
                    }
                    $CurrExpIdx = $LastExpIdx
                    #If first video, fade in.
                    if ($NFilesExported -eq 0)
                    {
                        $tdur = $file.srtdur
                        $CurrExpIdx = $CurrExpIdx+1
                        $tname = $tranprepend + "-fadein" + $postname + ".$GenFrmt"
                        $tnameNA = $tranprepend + "-fadein" + $postname + "NA.$GenFrmt"
                        $tincmd = "ffmpeg -y -f '$GenFrmt' -i `"$VSrt`" -vf `"fade=t=in:st=0:d=$tdur`" $EncodeDef `"$tname`""
                        #write-host $tincmd
                        #Invoke-Expression $tincmd
                        #Wait-Debugger
                        (Invoke-Expression $tincmd) *> $null
                        Start-Sleep -Seconds 0.2
                        $FileL = Get-ChildItem -Path "$tname" | Select-Object Length
                        #Wait-Debugger
                        if($FileL)
                        {
                            $VidPathStr[$CurrExpIdx] = "file `'$tname`'"
                            if (-not $IncAud)
                            {
                                $ffmpegcmd = "ffmpeg -y -f '$GenFrmt' -i `"$tname`"  -c copy -an `"$tnameNA`""
                                (Invoke-Expression $ffmpegcmd) *> $null
                                $VidPathStr[$CurrExpIdx] = "file `'$tnameNA`'"
                            }
                        }
                        else
                        {
                            Write-Error "Blank File"
                        }
                    }
                    #Else transition from previous video
                    else
                    {
                        #Wait-Debugger
                        $tdur = $file.srtdur
                        $CurrExpIdx = $CurrExpIdx+1
                        $tname = $tranprepend + $prename + "to" + $postname + ".$GenFrmt"
                        $tnameNA = $tranprepend + $prename + "to" + $postname + "NA.$GenFrmt"
                        $V1 = $PrevVid2TransitionFrom+"end"
                        #$tcmd = "ffmpeg -y -f '$GenFrmt' -i `"$V1`" -f '$GenFrmt' -i `"$VSrt`" -filter_complex `"[0:v][1:v]xfade=offset=0.0:duration=$tdur[vfade];[0:a][1:a]acrossfade=duration=$tdur[afade]`" -map vfade:v -map afade:a $EncodeDef `"$tname`""

                        $FSrt = "ffmpeg  -hide_banner -loglevel error -nostats -y -f lavfi -i"
                        $FAudIn = " anullsrc=r=$selarate`:d=$tdur"
                        $FAudOut =  " -map 0:a"
                        $tcmd = $FSrt + $FAudIn + " -f '$GenFrmt' -i `"$V1`" -f '$GenFrmt' -i `"$VSrt`" -filter_complex `"[1:v][2:v]xfade=offset=0.0:duration=$tdur[vfout]`" -map `"[vfout]`""+$FAudOut+" $EncodeDef `"$tname`""
                        if($CurrIdx -eq 13)
                        {
                            #write-host "ChkHere"
                        }
                        #write-host $tname
                        (Invoke-Expression $tcmd) *> $null
                        $FileL = Get-ChildItem -Path "$tname" | Select-Object Length
                        if($FileL)
                        {
                            if (-not $IncAud)
                            {
                                $ffmpegcmd = "ffmpeg -y -f '$GenFrmt' -i `"$tname`"  -c copy -an `"$tnameNA`""
                                (Invoke-Expression $ffmpegcmd) *> $null
                                $VidPathStr[$CurrExpIdx] = "file `'$tnameNA`'"
                            }
                            else
                            {
                                $VidPathStr[$CurrExpIdx] = "file `'$tname`'"
                            }
                        }
                        else
                        {
                            Write-Error "Blank File"
                        }
                    }
                    #Standard, just add the file to the transition list.
                    $FileL = Get-ChildItem -Path "$file" | Select-Object Length
                    if($FileL)
                    {
                        $CurrExpIdx = $CurrExpIdx+1
                        $VidPathStr[$CurrExpIdx] = "file `'$tname`'"
                        if (-not $IncAud)
                        {
                            $dirname = [System.IO.Path]::GetFileNameWithoutExtension($PrevVid2TransitionFrom)
                            $tnameNA = $tranprepend + $dirname + "NA.$GenFrmt"
                            $ffmpegcmd = "ffmpeg -y -f '$GenFrmt' -i `"$file`"  -c copy -an `"$tnameNA`""
                            (Invoke-Expression $ffmpegcmd) *> $null
                            $VidPathStr[$CurrExpIdx] = "file `'$tnameNA`'"
                        }
                        else
                        {
                            $VidPathStr[$CurrExpIdx] = "file `'$file`'"
                        }
                    }
                    else
                    {
                        Write-Error "Blank File"
                    }

                    #If the last file, fade to black.
                    if ($CurrIdx -eq $FileList.Count)
                    {
                        $tdur = ($file.enddur*0.9)
                        $CurrExpIdx = $CurrExpIdx+1
                        $tname = $tranprepend + "-fadeout" + $postname + ".$GenFrmt"
                        $tnameNA = $tranprepend + "-fadeout" + $postname + "NA.$GenFrmt"
                        $toutcmd = "ffmpeg -y -f '$GenFrmt' -i `"$VEnd`" -vf `"fade=t=out:st=0:d=$tdur`" $EncodeDef `"$tname`""
                        (Invoke-Expression $toutcmd) *> $null
                        $VidPathStr[$CurrExpIdx] = "file `'$tname`'"
                        $FileL = Get-ChildItem -Path "$tname" | Select-Object Length
                        if($FileL)
                        {
                            if (-not $IncAud)
                            {
                                $ffmpegcmd = "ffmpeg -y -f '$GenFrmt' -i `"$tname`"  -c copy -an `"$tnameNA`""
                                (Invoke-Expression $ffmpegcmd) *> $null
                                $VidPathStr[$CurrExpIdx] = "file `'$tnameNA`'"
                            }
                            else
                            {
                                $VidPathStr[$CurrExpIdx] = "file `'$file`'"
                            }
                        }
                        else
                        {
                            Write-Error "Blank File"
                        }
                    }
                    #If we get this far, update the previous properties for the next video to transition from.
                    $PrevVidDuration = $file.dur
                    $PrevVid2TransitionFrom = $file
                    $LastExpIdx = $CurrExpIdx
                    $NFilesExported++

                    #Wait-Debugger
                }
                catch{
                    $errorlogpath = $grpfldr+"_errorlog.txt"
                    $errorMessage = $_.Exception.Message
                    $errorDetails = $_.ErrorDetails
                    $failedItem = $_.TargetObject
                    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"

                    # Writing to a file
                    $logEntry = "*****Failed conversion*****" + `
                    "$timestamp - Error: $errorMessage - Details: $errorDetails - Item: $failedItem" + `
                    " **************************** "
                    Add-Content -Path $errorlogpath -Value $logEntry
                }

            }
            #Wait-Debugger
            $VidPathExp = $VidPathStr[0..$LastExpIdx]
            $VidPathExp| Out-File -FilePath "$appendlist" -force
            #Build list of all raw files to concat.
            $ffmpegcmd = "ffmpeg -y -safe 0 -f concat -i `"$appendlist`" -c copy -movflags faststart `"$finfile`""
            #Wait-Debugger
            $ffmpegcmd | Out-File -FilePath $buildcmd
            (Invoke-Expression $ffmpegcmd) *> $null

            #Concat files.
            Write-Host "$finfile complete"
        }

    }
    catch{
        $errorlogpath = $grpfldr+"_errorlog.txt"
        $errorMessage = $_.Exception.Message
        $errorDetails = $_.ErrorDetails
        $failedItem = $_.TargetObject
        $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"

        # Writing to a file
        $logEntry = "$timestamp - Error: $errorMessage - Details: $errorDetails - Item: $failedItem"
        Add-Content -Path $errorlogpath -Value $logEntry
    }
    finally{
        #Wait-Debugger
        Start-Sleep -Seconds 0.2
        #(Remove-Item -Path $grpfldr -Recurse -Force -EA SilentlyContinue -Verbose)*>$null
    }
}