function Join-VidPartsFromList
{
    param (
        $FileListProps,
        [string]$outputFile = "output",
        $GenFrmt = ".ts",
        [pscustomobject]$FrmtDef = [pscustomobject]@{OutFormat = "mp4";OutQuality = 20;ExpAud = 0;FPS = 30},
        $ReencodeOpt = ""
    )
    $OutFrmt = $FrmtDef.OutFormat
    $vidqty  = $FrmtDef.OutQuality
    $IncAud  = $FrmtDef.ExpAud
    $SelFPS  = $FrmtDef.FPS
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
    $finfile = $outputFile+".$OutFrmt"
    $intermittentfile = $outputFile+".tmp.$OutFrmt"
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
            $FileList | Add-Member -MemberType NoteProperty -Name ResDef -Value $([System.ValueTuple[int, int]])
            $FileList | Add-Member -MemberType NoteProperty -Name Dur -Value $([decimal])
            $FileList | Add-Member -MemberType NoteProperty -Name SrtDur -Value $([decimal])
            $FileList | Add-Member -MemberType NoteProperty -Name EndDur -Value $([decimal])
            $FileList | Add-Member -MemberType NoteProperty -Name FrameRate -Value $([decimal])
            $FileList | Add-Member -MemberType NoteProperty -Name AllowImport -Value $([int]1)
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
                $filename = [System.IO.Path]::GetFileNameWithoutExtension($entry)
                $parentPath = Split-Path $entry -Parent
                $dirpath = Join-Path $parentPath $filename
                $SrtChkName = $dirpath+".srt.$GenFrmt"
                $NomChkName = $dirpath+".$GenFrmt"
                $EndChkName = $dirpath+".end.$GenFrmt"
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
                $entry.ResDef = [System.ValueTuple[int, int]]::new(
                        $Width, $Height)

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
            if( -not ([string]::IsNullOrEmpty($ReencodeOpt))){
                $GrpSets = $FileList | Group-Object -Property VidDef
                $RencodeSet = 0
            }
            else{
                $GrpSets = $FileList | Group-Object -Property ResDef
                $RencodeSet = 1
            }
            #Get the output encoding from the group with the most files
            $GrpSets | Add-Member -MemberType NoteProperty -Name NFiles -value $([decimal]0)
            foreach ($Grp in $GrpSets)
            {
                foreach ($file in ($Grp| Select-Object -Expand Group))
                {
                    $Grp.NFiles = $Grp.NFiles + 1
                }
            }
            $GrpSets = $GrpSets| Sort-Object NFiles -Descending
            foreach ($grp in $GrpSets){
                if($NotInFirstGrp)
                {
                    #($file in ($_| Select-Object -Expand Group))
                    foreach($file in ($grp| Select-Object -ExpandProperty Group))
                    {
                        $file.AllowImport = 0
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
            if($RencodeSet)
            {
                $FinEncodeDef = "-video_track_timescale $vseltimebase -vcodec $selvcodec -crf $vidqty -preset slow -pix_fmt $selpixfmt -colorspace $selcolorspace -movflags faststart "
                $EncodeDef = "-video_track_timescale $vseltimebase -vcodec $selvcodec -crf 17 -preset slow -pix_fmt $selpixfmt -colorspace $selcolorspace -movflags faststart "
            }
            $FileList = @($GrpSets |  Select-Object -ExpandProperty Group) | Where-Object -Property AllowImport -eq 1
            $FileList | Add-Member -MemberType NoteProperty -Name ExportSuccess -Value $([int]0)
            $NFilesExported = 0
            $LastExpIdx = -1
            $CurrExpIdx = 0
            $PrevVid2TransitionFrom = ""
            $PVEnd = ""
            $VidPathStr = [String[]]::new($FileList.Count*2+1)
            $VidPathSet = [String[]]::new($FileList.Count*2+1)
            foreach ($file in $FileList)
            {
                $CurrIdx++
                if($CurrIdx -eq 36){
                    Write-Host "Chk"
                }
                try
                {
                    $postname = [System.IO.Path]::GetFileNameWithoutExtension($file)
                    $parentPath = Split-Path $file -Parent
                    $dirpath = Join-Path $parentPath $postname
                    $VSrt = $dirpath+".srt.$GenFrmt"
                    $NomChkName = $dirpath+".$GenFrmt"
                    $VEnd = $dirpath+".end.$GenFrmt"
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
                        $tincmd = "ffmpeg -y -i `"$VSrt`" -vf `"fade=t=in:st=0:d=$tdur`" $EncodeDef -c:a copy `"$tname`""
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
                            $VidPathSet[$CurrExpIdx] = "`"$tname`""
                            if (-not $IncAud)
                            {
                                $ffmpegcmd = "ffmpeg -y -i `"$tname`"  -c copy -an `"$tnameNA`""
                                (Invoke-Expression $ffmpegcmd) *> $null
                                $VidPathStr[$CurrExpIdx] = "file `'$tnameNA`'"
                                $VidPathSet[$CurrExpIdx] = "`"$tnameNA`""
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
                        $V1 = $PVEnd
                        #$tcmd = "ffmpeg -y -i `"$V1`" -i `"$VSrt`" -filter_complex `"[0:v][1:v]xfade=offset=0.0:duration=$tdur[vfade];[0:a][1:a]acrossfade=duration=$tdur[afade]`" -map vfade:v -map afade:a $EncodeDef `"$tname`""

                        $FSrt = "ffmpeg  -hide_banner -loglevel error -nostats -y -f lavfi -i"
                        $FAudIn = " anullsrc=r=$selarate"
                        $FAudOut =  " -map 0:a -c:a $selacodec -ar $selarate -shortest"
                        $tcmd = $FSrt + $FAudIn + " -i `"$V1`" -i `"$VSrt`" -filter_complex `"[1:v][2:v]xfade=offset=0.0:duration=$tdur[vfout]`" -map `"[vfout]`""+$FAudOut+" $EncodeDef `"$tname`""
                        if($CurrIdx -eq 36)
                        {
                            write-host "ChkHere"
                        }
                        #write-host $tname
                        (Invoke-Expression $tcmd) *> $null
                        $FileL = Get-ChildItem -Path "$tname" | Select-Object Length
                        if($FileL)
                        {
                            if (-not $IncAud)
                            {
                                $ffmpegcmd = "ffmpeg -y -i `"$tname`"  -c copy -an `"$tnameNA`""
                                (Invoke-Expression $ffmpegcmd) *> $null
                                $VidPathStr[$CurrExpIdx] = "file `'$tnameNA`'"
                                $VidPathSet[$CurrExpIdx] = "`"$tnameNA`""
                            }
                            else
                            {
                                $VidPathStr[$CurrExpIdx] = "file `'$tname`'"
                                $VidPathSet[$CurrExpIdx] = "`"$tname`""
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
                        $VidPathSet[$CurrExpIdx] = "`"$tname`""
                        if (-not $IncAud)
                        {
                            $dirname = [System.IO.Path]::GetFileNameWithoutExtension($PrevVid2TransitionFrom)
                            $tnameNA = $tranprepend + $dirname + "NA.$GenFrmt"
                            $ffmpegcmd = "ffmpeg -y -i `"$file`"  -c copy -an `"$tnameNA`""
                            (Invoke-Expression $ffmpegcmd) *> $null
                            $VidPathStr[$CurrExpIdx] = "file `'$tnameNA`'"
                            $VidPathSet[$CurrExpIdx] = "`"$tnameNA`""
                        }
                        else
                        {
                            $VidPathStr[$CurrExpIdx] = "file `'$file`'"
                            $VidPathSet[$CurrExpIdx] = "`"$tname`""
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
                        $toutcmd = "ffmpeg -y -i `"$VEnd`" -vf `"fade=t=out:st=0:d=$tdur`" $EncodeDef `"$tname`""
                        (Invoke-Expression $toutcmd) *> $null
                        $VidPathStr[$CurrExpIdx] = "file `'$tname`'"
                        $VidPathSet[$CurrExpIdx] = "`"$tname`""
                        $FileL = Get-ChildItem -Path "$tname" | Select-Object Length
                        if($FileL)
                        {
                            if (-not $IncAud)
                            {
                                $ffmpegcmd = "ffmpeg -y -i `"$tname`"  -c copy -an `"$tnameNA`""
                                (Invoke-Expression $ffmpegcmd) *> $null
                                $VidPathStr[$CurrExpIdx] = "file `'$tnameNA`'"
                                $VidPathSet[$CurrExpIdx] = "`"$tnameNA`""
                            }
                            else
                            {
                                $VidPathStr[$CurrExpIdx] = "file `'$file`'"
                                $VidPathSet[$CurrExpIdx] = "`"$tname`""
                            }
                        }
                        else
                        {
                            Write-Error "Blank File"
                        }
                    }
                    #If we get this far, update the previous properties for the next video to transition from.
                    $PrevVid2TransitionFrom = $file
                    $PVEnd = $VEnd
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
            $VidPathSelSet = $VidPathSet[0..$LastExpIdx]
            #Build list of all raw files to concat.
            $VidPathExp| Out-File -FilePath "$appendlist" -force
            #Define build command, depend on if reencoding or not.
            if($RencodeSet){
                $find = -1
                $FiltStr = "`""
                $FileInputStr = ""
                $N2Concat = 0
                foreach ($file2con in $VidPathSelSet){
                    $find++
                    $N2Concat++
                    $FileInputStr+=" -i $file2con"
                    $FiltStr+= "[$find`:v:0]"
                    if($IncAud){
                        FiltStr+= "[$find`:a:0]"
                    }
                }
                if($FrmtDef.XDim -ne $PreWidth){
                $resizew = $FrmtDef.XDim}
                else{$resizew = $null}
                if($FrmtDef.YDim -ne $PreHeight){
                $resizeh = $FrmtDef.YDim}
                else{$resizeh = $null}
                if($resizew -and $resizeh){
                    $rszcfg = $resizew.ToString()+"`:"+$resizeh.ToString()
                }
                elseif($resizew ){
                    $rszcfg = $resizew.ToString()+"`:-1"
                }
                elseif($resizeh){
                    $rszcfg = "-1`:"+$resizeh.ToString()
                }
                else{
                    $rszcfg = ""
                }
                #Complete the concat string.
                $FiltStr+="concat=n=$N2Concat`:v=1"
                if($IncAud){
                    FiltStr+= ":a=1[outv][outa]"
                }
                else{
                    $FiltStr+= "[outv]"
                }
                #Add in the resize filter if defined, else just map what's available.
                if([string]::IsNullOrEmpty($rszcfg)){
                    $rszstr = ""
                    if($IncAud){
                        FiltStr+= "`" -map `"[outv]`" -map `"[outa]`""
                    }
                    else{
                        $FiltStr+= "`" -map `"[outv]`""
                    }
                }
                else{
                    $rszstr = "`;[outv]scale="+$rszcfg+"[sclv]"
                    if($IncAud){
                        FiltStr+= "$rszstr`" -map `"[sclv]`" -map `"[outa]`""
                    }
                    else{
                        $FiltStr+= "$rszstr`" -map `"[sclv]`""
                    }
                }
                $ffmpegcmd = "ffmpeg -y $FileInputStr -filter_complex $FiltStr $FinEncodeDef `"$finfile`""
                $2ndPass = $false
            }
            else{
                $ffmpegcmd = "ffmpeg -y -safe 0 -f concat -i `"$appendlist`" -c copy -movflags faststart `"$intermittentfile`""
                $2ndPass = $true
            }
            #Wait-Debugger
            $ffmpegcmd | Out-File -FilePath $buildcmd
            if($ffmpegcmd.length -gt 32766){
                Write-Error "Command length limit reached, reduce the number of videos (via BulkVidTimeMin) or update this script to perform 2-step concat"
            }
            else{
                (Invoke-Expression $ffmpegcmd) *> $null
                if ($2ndPass){
                    $Outdef = [pscustomobject]@{OutQuality = $vidqty}
                    Convert-Video $intermittentfile $finfile $Outdef
                    Remove-Item $intermittentfile -Force
                }
            }

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
        (Remove-Item -Path $grpfldr -Recurse -Force -EA SilentlyContinue -Verbose)*>$null
    }
}

function Convert-Video
{
    param (
        $VideoInPath,
        $VideoOutPath,
        $VideoOutProps = ""
    )
    #Get video properties to determine if resize filter is required
    $VPramsCmd = "ffprobe -v error -show_streams -select_streams v`:0 -of ini `"$VideoInPath`""
    $VPrams = Invoke-Expression $VPramsCmd
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
        }
    }
    if($VideoOutProps.XDim){
        if($PreWidth -ne $VideoOutProps.XDim){
            $resizew=$VideoOutProps.XDim
        }
    }
    if($VideoOutProps.YDim){
        if($PreHeight -ne $VideoOutProps.YDim){
            $resizeh=$VideoOutProps.YDim
        }
    }
    if($VideoOutProps.OutQuality){
        $SelQual = $VideoOutProps.OutQuality;
    }
    else{
        $SelQual = 20
    }
    if($resizew -and $resizeh){
        $rszcfg = $resizew.ToString()+"`:"+$resizeh.ToString()
    }
    elseif($resizew ){
        $rszcfg = $resizew.ToString()+"`:-1"
    }
    elseif($resizeh){
        $rszcfg = "-1`:"+$resizeh.ToString()
    }
    else{
        $rszcfg = ""
    }
    $filterdefs = @()
    if([string]::IsNullOrEmpty($rszcfg)){
        $rszstr = ""
    }
    else{
        $rszstr = "scale="+$rszcfg
        $filterdefs += @($rszstr)
    }
    if ($filterdefs.count){
        $vfstr = "-vf `"$($filterdefs | Join-String -Separator ",")`""
    }
    else
    {
        $vfstr = ""
    }
        #Setup filter according to definition
        $ffmpgcmdsw = "ffmpeg -y -vsync 0 -i $VideoInPath $vfstr -vcodec h264 -crf $SelQual -preset slow $VideoOutPath"
        (Invoke-Expression $ffmpgcmdsw) *> $null
        #Execute

        #NVidia ref 1:
        #ffmpeg -y -vsync 0 -hwaccel cuda -hwaccel_output_format cuda –resize 1280x720 -i input.mp4 -c:a copy -c:v h264_nvenc -b:v 5M output.mp4

        #cuda not recommended, but might be necessary for older GPUs, might find a way to set this in an outcome of a try condition.
        #ffmpeg -y -vsync 0 -hwaccel cuda -hwaccel_output_format cuda -i input.mp4 -vf scale_cuda=1280:720 -c:a copy -c:v h264_nvenc -b:v 5M output.mp4

        #NVidia ref 2:
        # ffmpeg -y -vsync 0 -hwaccel cuda -hwaccel_output_format cuda -i input.mp4 -vf scale_npp=1280:720 -c:a copy -c:v h264_nvenc -b:v 5M output.mp4


}