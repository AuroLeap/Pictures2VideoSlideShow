Clear-Host #Process level on next line: 0 = all, 1 = move to process path, 2 = convert from process path to output
$PSVersionFnd = $PSVersionTable.PSVersion
if ($PSVersionFnd -lt 7.1)
{
    Write-Host "Reported Powershell Version: $($PSVersionFnd.ToString())"
    Write-Host "Use command `"pwsh`" to call newer versions of powershell, update / install by using `"winget install --id Microsoft.PowerShell --source winget`" "
    Throw "Powershell must be version 7.1 or greater"
}
Write-Host "Powershell version: $($PSVersionTable.PSVersion)"
Write-Host $PSScriptRoot
Set-Location -Path $PSScriptRoot
$ProcLvl = 0 #Usually 0 (Process all) unless debugging.
$SetTmpPath = "" #"T" #If utalizing RAM drive for conversion (1 gb), set this to the letter of the drive that should be created.  Else keep blank.
$GenFrmt = "ts"
$ReencodeOpt = "libx264"

if ((HOSTNAME) -EQ "DESKTOP-OFFICE")
{
    $BuDrv = "Z"
    $SetTmpPath = "T:"
}
else
{
    $BuDrv = "D"
    $SetTmpPath = ""
}
#For testing, note the configuration file is also
$UseTestPath = 1;
$DemoPrepAndConvPaths = 0;
if ($UseTestPath)
{
    $Names2Ig = @(@{
        Name = "DNP"
        ExceptionParentFldrGreaterThan = [double]::PositiveInfinity
    }
    @{
        Name = "DNP2"
        ExceptionParentFldrGreaterThan = [double]::PositiveInfinity
    })
    if($DemoPrepAndConvPaths)
    {
       $InputFileRootPath = ".\TestInput"
       $PrepFileRootPath  = ".\TestOut\Prep"
       $ConvFileRootPath  = ".\TestOut\Conv"
    }
    else
    {
       $InputFileRootPath = ".\TestInput"
       $PrepFileRootPath  = ".\TestOut\Prep"
       $ConvFileRootPath  = ""
    }
    $OutputFilePrepend = ".\AlbumOut"
    $OutputDefs = @(
    [pscustomobject]@{
        XDim = 1440;
        YDim = 900;
        FPS = 20;
        PicDispTime = 6;
        MaxSrtRot = 30;
        FadeTime = 0.7;
        BulkVidTimeMin = 0.3;
        NameMethod = "FldrLvl2";
        ImgVidFldr = "\ImgInVid";
        Quality = 30;
        ExpAud = 0; #Note: Keep 0 until / unless fixed; exporting audio doesn't appear to work (information becomes corrupted, video playback freezes)
    })

}
#************************************
#Define standard / non-test paths
#************************************
else
{
    $Names2Ig = @(@{
        Name = "DNP"
        ExceptionParentFldrGreaterThan = [double]::PositiveInfinity
    }
    @{
        Name = "JohnsonFamilySide"
        ExceptionParentFldrGreaterThan = 2015
    }
    )
    $InputFileRootPath ="S:"
    $PrepFileRootPath  = $BuDrv+ ":\AlbumPrep"
    #$ConvFileRootPath  = $BuDrv+ ":\AlbumConv"
    $ConvFileRootPath  = ""
    $OutputFilePrepend = $BuDrv+ ":\Album"
    $OutputDefs = @(
        #[pscustomobject]@{
        #    XDim = 1440;
        #    YDim = 900;
        #    FPS = 30;
        #    PicDispTime = 6;
        #    MaxSrtRot = 20;
        #    FadeTime = 1;
        #    BulkVidTimeMin = 4;
        #    NameMethod = "FldrLvl2";
        #    ImgVidFldr = "\ImgInVid";
        #    Quality = 23;
        #    ExpAud = 0; #Note: Keep 0 until / unless fixed; exporting audio doesn't appear to work (information becomes corrupted, video playback freezes)
        #}
        [pscustomobject]@{
            XDim = 1920;
            YDim = 1080;
            Quality = 22;
            FPS = 30;
            PicDispTime = 6;
            MaxSrtRot = 20;
            FadeTime = 0.7;
            BulkVidTimeMin = 4;
            NameMethod = "FldrLvl2";
            ImgVidFldr = "\ImgInVid";
            ExpAud = 0; #Note: Keep 0 until / unless fixed; exporting audio doesn't appear to work (information becomes corrupted, video playback freezes)
            #Define any additional sets which are just copies of this definition, but resized.  These should always be lower resolution than the original.
            AdditionalOutputs = @(
                [pscustomobject]@{
                    XDim = 1440;
                    YDim = 900;
                    Quality = 22;
                }
            )
        }
    )
}
#Clean paths if applicable
$CopyMedia = 0

#Adding dependent scripts:
Import-Module ".\CopyMediaFromNetwork2Local.psm1" -Force
Import-Module ".\PrepareMediaForDisplay.psm1" -Force
Import-Module ".\PackMediaIntoVideo.psm1" -Force

#Build derived definitions
$OutputDefs | Add-Member -MemberType NoteProperty -Name Outpath -Value $([string])
$OutputDefs | Add-Member -MemberType NoteProperty -Name OutGrp -Value $([string])
$OutputDefs | Add-Member -MemberType NoteProperty -Name InputVideoPath -Value $([string])
$OutputDefs | Add-Member -MemberType NoteProperty -Name VidPack -Value $([Int])
$DirSepChar = [System.IO.Path]::DirectorySeparatorChar
$DerivedSets = @()
#Create derived group properties, and create the derived set group for group creation from common media segments.
foreach($set in $OutputDefs)
{
    $Prepend  = [System.IO.Path]::GetFullPath($OutputFilePrepend, (Get-Location).Path)
    $BulkDef  = "_FPS"+$set.FPS+"_DT"+$set.PicDispTime+"_FT"+$set.FadeTime+"_MR"+$set.MaxSrtRot
    $Set.Outpath = ($Prepend+$set.XDim+"x"+$set.YDim+$BulkDef + "q"+$set.Quality)
    $Set.OutGrp = ($Set.Outpath+"-Groups")
    if($Set.PicDispTime -and $Set.BulkVidTimeMin -and $Set.ImgVidFldr.Count)
    {
        $Set.VidPack = 1
    }
    $NewSet = $Set | Select-Object -ExcludeProperty AdditionalOutputs
    $NewSet.InputVideoPath = ""
    $DerivedSets += @($NewSet)
    foreach($subset in $Set.AdditionalOutputs){
        $NewSet = $Set | Select-Object -ExcludeProperty AdditionalOutputs
        if($subset.Quality){
            $SubQuality = $subset.Quality
        }
        else{
            $SubQuality = $set.Quality
        }
        $NewSet.Quality = $SubQuality
        $NewSet.XDim = $subset.XDim
        $NewSet.YDim = $subset.YDim
        $PreName = $Prepend+$NewSet.XDim+"x"+$NewSet.YDim+$BulkDef + "q"+$SubQuality
        $NewSet.OutGrp = ($PreName+"-Groups")
        $NewSet.InputVideoPath = $Set.OutGrp
        $DerivedSets += @($NewSet)
    }
}
#Root path:
$RtFnd = Test-Path -LiteralPath $InputFileRootPath
if($InputFileRootPath.Length -and (-not ($RtFnd )))
{
    Throw "Root folder defined but not found, note if this is a mapped drive, the admin workspace may need this mapped through UNC by using the command in an elevated command prompt `"net use <MappedDrv> <Network Path>`""
}
elseif ($InputFileRootPath.Length)
{
    $InputFileRootPath = (Resolve-Path $InputFileRootPath).Path
    #Append file seperation character if not present for consistency.
    if($InputFileRootPath[-1] -ne $DirSepChar)
    {$InputFileRootPath = $InputFileRootPath+$DirSepChar}
}
else
{
    $InputFileRootPath = ""
    Write-Host "Root path not found or not defined, tool will consider defined prep path as source"
}
#Prep path:
if (Test-Path $PrepFileRootPath)
{
}
elseif ($PrepFileRootPath.Length)
{
    New-Item -ItemType Directory $PrepFileRootPath
}
else
{
    Throw "Prep path not defined, no action can be taken without media content path defined"
}
$PrepFileRootPath  = (Resolve-Path $PrepFileRootPath).Path
if($PrepFileRootPath[-1] -ne $DirSepChar)
{
    $PrepFileRootPath = $PrepFileRootPath+$DirSepChar
}
#Conversion path:
if ($ConvFileRootPath.Length)
{
    if (Test-Path $ConvFileRootPath) {}
    else{New-Item -ItemType Directory $ConvFileRootPath}
    $ConvFileRootPath  = (Resolve-Path $ConvFileRootPath).Path
    if($ConvFileRootPath[-1] -ne $DirSepChar)
    {$ConvFileRootPath = $ConvFileRootPath+$DirSepChar}
}
else
{
    Write-Host "Conversion path is not defined, assuming it is not intended, no conversion will be performed, files will be taken directly from the prep path."
    $ConvFileRootPath = ""
}
#Run operations.
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
if (($ProcLvl -eq 0) -or ($ProcLvl -eq 1) -or $CopyMedia){
    Copy-MediaFromNetwork $InputFileRootPath $PrepFileRootPath $Names2Ig
}
if (($ProcLvl -eq 0) -or ($ProcLvl -eq 2)){
    $PrepMediaDef = Update-ConvertedMediaImagesForDisplay $GenFrmt $PrepFileRootPath $ConvFileRootPath
    Update-MediaForDisplaySets $GenFrmt $PrepMediaDef $OutputDefs $SetTmpPath
}
if (($ProcLvl -eq 0) -or ($ProcLvl -eq 3)){
    Write-Host "***************************************"
    Write-Host "**** Building final output videos *****"
    Write-Host "***************************************"
    Set-VideoFromMedia $GenFrmt $DerivedSets $ReencodeOpt
}
$stopwatch.Stop()
$elapsedTime = $stopwatch.Elapsed
write-host "Elapsed time: $elapsedTime"
Read-Host -Prompt "Press enter to continue..."


