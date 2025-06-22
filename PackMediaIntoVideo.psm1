$UseParallel = 0
$ConcatModulePath = ".\ConcatVidPartsFromFileList.psm1"
$RingShiftRatio = 0
$DepPath = Resolve-Path $ConcatModulePath
Import-Module $DepPath
function Set-VideoFromMedia
{
    param (
        $GenFrmt,
        $OutputSizes,
        $ReencodeOpt = ""
    )

    if (Get-Command ffmpeg -ErrorAction SilentlyContinue) {
    #Write-Host "ffmpeg is already installed."
    }
    else{Throw "ffmpeg not detected, videos will not be converted" }

    #Now - for each set - break all the files into designated groups and their corresponding destinations.
    foreach ($set in $OutputSizes)
    {
        $ContFileRootPath = $set.Outpath
        $VidPacksRootPath = $ContFileRootPath + $set.ImgVidFldr
        $VidPacksFileDefPath = $set.OutGrp
        #If we have a video input path, then we need to just user that as the input.
        if (-not ([string]::IsNullOrEmpty($set.InputVideoPath)))
        {
            $Vids2Convert = @(Get-ChildItem -LiteralPath $set.InputVideoPath -Filter "*.$($set.OutFormat)")
            $Vids2Convert | Add-Member -MemberType NoteProperty -Name OutPath -Value $( [string] )
            if(-not (Test-Path -Path $VidPacksFileDefPath -Type Container)){
                New-Item -ItemType Directory -Path $VidPacksFileDefPath
            }
            foreach($VidFile in $Vids2Convert){
                $VidFile.OutPath = Join-Path -Path $VidPacksFileDefPath -ChildPath $VidFile.Name
            }
            if ($UseParallel)
            {
                $Vids2Convert | ForEach-Object -Parallel{
                    $DepPath = $using:DepPath
                    Import-Module $DepPath
                    $set = $using:set
                    $Vid2Convert = $_
                    Convert-Video $Vid2Convert.FullName $Vid2Convert.OutPath $set
                } -ThrottleLimit 4
            }
            else
            {
                foreach($Vid2Convert in $Vids2Convert){
                    Convert-Video $Vid2Convert.FullName $Vid2Convert.OutPath $set
                }
            }
        }
        #If there is no input video path, then the intent is to build video sets from the raw input video segments and stitch together.
        else
        {
            $PrepAllInputFiles = @(Get-ChildItem -LiteralPath $ContFileRootPath -Filter "*.$GenFrmt")
            #Add in video packs for images if defined and intended.
            if ((Test-Path -LiteralPath $VidPacksRootPath -PathType Container) -and $set.ImgVidFldr.Length -and $set.PicDispTime)
            {
                $PrepAllInputFiles = $PrepAllInputFiles + @(Get-ChildItem -LiteralPath $VidPacksRootPath -Filter "*.$GenFrmt")
            }
            $PrepAllInputFiles = $PrepAllInputFiles | Where-Object { !$_.Name.EndsWith("end.$GenFrmt") -and !$_.Name.EndsWith("srt.$GenFrmt") }
            $PrepAllInputFiles | Add-Member -MemberType NoteProperty -Name GroupN -Value $( [int] )
            $PrepAllInputFiles | Add-Member -MemberType NoteProperty -Name Dur -Value $( [Decimal] )
            $PrepAllInputFiles | Add-Member -MemberType NoteProperty -Name FileInd -Value $( [int] )
            #Shift the set of input files.
            if ($PrepAllInputFiles.Count -lt 2)
            {
                $AllInputFiles = $PrepAllInputFiles
            }
            else
            {
                $NFiles2Shift = [Math]::Round($PrepAllInputFiles.Count*$RingShiftRatio)
                if ($NFiles2Shift -eq 0)
                {
                    $NFiles2Shift = 1
                }
                $AllInputFiles = $PrepAllInputFiles[$NFiles2Shift..$PrepAllInputFiles.Count] + $PrepAllInputFiles[0..($NFiles2Shift - 1)]
            }
            #Figure out the nominal number of files per group, assuming most are pictures lasting for the still duration.
            $NFilesPerGrp = (($Set.BulkVidTimeMin*60)/$Set.PicDispTime)
            $NGroups = [Math]::Floor($AllInputFiles.Count/$NFilesPerGrp) -as [Int]
            $SelGrpN = 1 -as [Int]
            foreach ($file in $AllInputFiles)
            #For each file
            {
                if ($file.FullName.EndsWith(".srt.mp4") -or $file.FullName.EndsWith(".srt.mp4"))
                {
                #Do nothing, should not be added to a positve group number.
                }
                else
                {
                    $file.GroupN = $SelGrpN
                    $SelGrpN++
                    $SetFileInd++
                    $file.FileInd = $SetFileInd
                    if ($SelGrpN -gt $NGroups)
                    {
                        $SelGrpN = 1
                    }
                }
                #Leaving in case duration is needed in the future for grouping
                #$ToRun = "ffprobe -v error -select_streams v -show_entries stream=width,duration -of csv=p=0 `"" +$file.Fullname+ "`""
                #($VPrams = Invoke-Expression $ToRun) *> $null
                #$splitString = $VPrams -split ","
                ##$Width = [Int] $splitString[0]
                #$file.Dur = [decimal] $splitString[1]
            }
            #if ($set.MaxSizeInGB)
            #{
            #    $DurTotal = $AllInputFiles | Measure-Object -Property Dur -Sum ; $DurTotal = [double] $DurTotal.Sum
            #    #$BitTotal = 8*$set.MaxSizeInGB*1.25e+8
            #    #$TarBitrate = $BitTotal/$DurTotal #Assume fading is negligable.
            #}
            $Groups = $AllInputFiles | Where-Object { $_.GroupN -ne 0 } | Select-Object -ExpandProperty GroupN | Sort-Object -Unique
            $Groups | Add-Member -MemberType NoteProperty -Name FileListPath -Value $( [string] )
            $Groups | Add-Member -MemberType NoteProperty -Name VidExpPath -Value $( [string] )
            #Map to store file list.
            $GrpDef = @{ }
            #Now build a list for each group.  This will be used in ffmpeg to actually build out the video.
            Write-Host "***********************************************"
            Write-Host "Exporting $($NGroups.ToString() ) groups..."
            Write-Host "***********************************************"
            foreach ($grp in $Groups)
            {
                $FileSet = (($AllInputFiles | Where-Object { ( $_.GroupN -eq $grp) } | Sort-Object -Property Name) | Select-Object -ExpandProperty FullName)
                $GrpDef[$grp] = $FileSet
                $grp.FileListPath = $VidPacksFileDefPath + " Grp-" + $grp.ToString()
                $grp.VidExpPath = $VidPacksFileDefPath + "\Grp-" + $grp.ToString()
                #Create file to describe what videos to append.
                #$FileSet | Export-Csv -Path $grp.FileListPath -NoTypeInformation
            }
            if ($UseParallel)
            {
                $Groups | ForEach-Object -Parallel{
                    $DepPath = $using:DepPath
                    $locset = $using:set
                    $GrpDef = $using:GrpDef
                    Import-Module $DepPath
                    $SelGrpDef = $GrpDef[$_]
                    $GenFrmt = $using:GenFrmt
                    if ($locset.UseHQIntermittents)
                    {
                        $ReencodeOpt = $using:ReencodeOpt
                    }
                    else
                    {
                        $ReencodeOpt = ""
                    }

                    Join-VidPartsFromList $SelGrpDef $_.VidExpPath $GenFrmt $locset $ReencodeOpt
                } -ThrottleLimit 4
            }
            else
            {
                foreach ($grp in $Groups)
                {
                    $SelGrpDef = $GrpDef[$grp]
                    $SelVidExpPath = $grp.VidExpPath
                    if ($set.UseHQIntermittents)
                    {
                        $SelReencodeOpt = $ReencodeOpt
                    }
                    else
                    {
                        $SelReencodeOpt = ""
                    }
                    Join-VidPartsFromList $SelGrpDef $SelVidExpPath $GenFrmt $set $SelReencodeOpt
                }
            }
            $SelGrpN = 0
        }
    }
}
