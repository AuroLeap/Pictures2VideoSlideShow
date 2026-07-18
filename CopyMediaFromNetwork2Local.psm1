function Copy-MediaFromNetwork
{
    param (
        [string]$inputFolder,
        [string]$outputFolder,
        $Names2Ig = @()
    )

    $Types2Pull = "jpg$","gif$","tif$","tiff$","jpeg$","png$","bmp$","wmv$","mov$","mp4$","avi$"
    $TypeChkRegex = [string]::Join('|', $Types2Pull)

    $CurrInnerProgPercInt = [int32[]]::new(1);
    $PrevInnerProgPercInt = [int32[]]::new(1);
    $InnerLoopProg = @{
	ID       = 1
	Activity = "Getting ready.  Please wait..."
	Status   = "Getting ready.  Please wait..."
	PercentComplete  = 0
	CurrentOperation = 0
    }

    #Verify all process staging files are up-to-date.  Copy them over or delete them as needed.
    if (Test-Path -LiteralPath $inputFolder)
    {
        #Make the prep directory if it doesn't already exist.
        if (Test-Path -LiteralPath $outputFolder)
        {
        }#do nothing
        else
        {
            New-Item -Path "$outputFolder" -ItemType Directory
        }

        $AllInputFiles = @(Get-ChildItem -LiteralPath $inputFolder -Recurse -File)
        #For all files, see if it should be copied, and if so if it already is.
        #Then create a tuple for the filename, size, and datetime.
        if ($AllInputFiles.Count)
        {
            $FndFile = @{ }
            $AllInputFiles | Add-Member -MemberType NoteProperty -Name CopyPath -Value $( [string] )
            $AllInputFiles | Add-Member -MemberType NoteProperty -Name CopyFlag -Value $( [int]0 )
            $AllInputFiles | Add-Member -MemberType NoteProperty -Name TupleVal -Value [System.ValueTuple[string, long, datetime]]
            $SrcL = $inputFolder.Length
            #Write-Host ($AllInputFiles.Count.ToString() + " files found!")

            foreach ($file in $AllInputFiles)
            {
                $IncChk = 1
                foreach($IgDef in $Names2Ig)
                {
                    if($file.FullName.Contains($IgDef.Name))
                    {
                        $IncChk = 0
                        #If it is set to ignore, is the folder in the exception range?  Then actually allow it
                        #Probably more graceful ways to do this...
                        $ParentFldrName = Split-Path (Split-Path -Parent $file.FullName) -Leaf
                        if (($ParentFldrName -match "\d+") -and (-not $ParentFldrName.Contains($IgDef.Name)))
                        {
                            # $matches[0] contains the first numeric substring found in $_
                            [int]$number = $matches[0]
                            if($number -ge $IgDef.ExceptionParentFldrGreaterThan)
                            {
                                $IncChk = 1
                            }
                        }
                        else {
                            #$false  # No number found, so filter out
                        }
                        if(-not $IncChk)
                        {
                            break
                        }
                    }
                }
                if ($IncChk -and ($file.Name -match $TypeChkRegex))
                {
                    $file.CopyFlag = 1
                }
                if ($file.CopyFlag)
                {
                    $ExtLen = $file.FullName.Length - $SrcL
                    $RelPth = $file.FullName.Substring($SrcL, $ExtLen)
                    $file.CopyPath = Join-Path -Path $outputFolder -ChildPath $RelPth
                    $datekey = [System.ValueTuple[string, long, datetime]]::new(
                            $RelPth, $file.Length, $file.LastWriteTime)
                    $FndFile[$datekey] = 1
                    $file.TupleVal = $datekey;
                }
            }
            $PreFiles2Copy = ($AllInputFiles | Where-Object -Property CopyFlag -eq 1)
            Write-Host ($PreFiles2Copy.Count.ToString() + " of " + $AllInputFiles.Count.ToString() + " media files found that match criteria!")
            #Now, for all prep files, see remove any that don't have a tuple match
            $PrpFndFile = @{ }
            $PrpL = $outputFolder.Length
            $PrePrepFiles = @(Get-ChildItem -LiteralPath $outputFolder -Recurse -File)
            foreach ($file in $PrePrepFiles)
            {
                $ExtLen = $file.FullName.Length - $PrpL
                $RelPth = $file.FullName.Substring($PrpL, $ExtLen)
                $datekey = [System.ValueTuple[string, long, datetime]]::new(
                        $RelPth, $file.Length, $file.LastWriteTime)
                $PrpFndFile[$datekey] = 1
                #if the file exists/matches the source, ignore it.
                if ($FndFile[$datekey])
                {
                }
                #else, the file isn't in the source anymore, and should be removed.
                else
                {
                    Remove-Item -LiteralPath $file.FullName
                }
            }
            #Now copy all the files that need to be copied.
            $PreFiles2Copy | Add-Member -MemberType NoteProperty -Name SkipFlg -Value $( [int]0 )
            foreach ($file in $PreFiles2Copy)
            {
                #If the file already exists in the prep path, don't do anything
                if ($PrpFndFile[$file.TupleVal])
                {
                    $file.SkipFlg = 1
                }
                #else copy it accordingly.
            }
            $Files2Copy = ($PreFiles2Copy | Where-Object -Property SkipFlg -eq 0)
            Write-Host ($Files2Copy.Count.ToString() + " media files to copy!")
            $PrevInnerProgPercInt[0] = 0
            $LoopProg = 0
            $AllFilesizeTtl = $Files2Copy | Measure-Object -Property Length -Sum; $AllFilesizeTtl = $AllFilesizeTtl.Sum

            foreach ($file in $Files2Copy)
            {
                if (Test-Path -LiteralPath $file.CopyPath)
                {
                }#Do nothing
                else
                {
                #Create file
                    $null = New-Item -ItemType File -Path $file.CopyPath -Force
                }
                Copy-Item $file.FullName $file.CopyPath -Force
                $LoopProg += $file.Length
                $CurrInnerProgPercInt[0] = ($LoopProg*100)/$AllFilesizeTtl
                if ($CurrInnerProgPercInt[0] -gt $PrevInnerProgPercInt[0])
                {
                    $InnerLoopProg.PercentComplete = $CurrInnerProgPercInt[0]
                    $PrevInnerProgPercInt[0] = $CurrInnerProgPercInt[0]
                    $InnerLoopProg.Status = "Copying files: " + $InnerLoopProg.PercentComplete.ToString() + "% Complete"
                    Write-Progress @InnerLoopProg
                }
            }
        }
        else
        {
            Write-Host "No files found in source path, tool will proceed to check prep path"
        }
    }
    #If we aren't set to process the source directory, just grab file definitoins from prep space.
    elseif(Test-Path -LiteralPath $outputFolder)
    {
        #Do nothing, proceed as is.
    }
    else
    {
        throw "The input processing path was empty."
    }
    write-host "File syncronization complete"
}