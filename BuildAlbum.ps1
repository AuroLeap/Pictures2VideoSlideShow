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
        ExpAud = 1;
        CleanBuild = 0;
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
    $ExceptionParentFldrGreaterThan = 2015
    $InputFileRootPath ="S:"
    $PrepFileRootPath  = $BuDrv+ ":\AlbumPrep"
    #$ConvFileRootPath  = $BuDrv+ ":\AlbumConv"
    $ConvFileRootPath  = ""
    $OutputFilePrepend = $BuDrv+ ":\Album"
    $OutputDefs = @(
        [pscustomobject]@{
            XDim = 1440;
            YDim = 900;
            FPS = 30;
            PicDispTime = 6;
            MaxSrtRot = 30;
            FadeTime = 1;
            BulkVidTimeMin = 20;
            NameMethod = "FldrLvl2";
            ImgVidFldr = "\ImgInVid";
            Quality = 23;
            ExpAud = 0;
            CleanBuild = 0;
        }
        #[pscustomobject]@{
        #    XDim = 1920;
        #    YDim = 1080;
        #    FPS = 30;
        #    PicDispTime = 6;
        #    MaxSrtRot = 30;
        #    FadeTime = 0.7;
        #    BulkVidTimeMin = 30;
        #    NameMethod = "FldrLvl2";
        #    ImgVidFldr = "\ImgInVid";
        #    Quality = 22;
        #    ExpAud = 0;
        #    CleanBuild = 0;
        #}
    )
}
#Clean paths if applicable
$CopyMedia = 0
foreach ($def in $OutputDefs)
{
    if ($def.CleanBuild -and $InputFileRootPath.Length)
    {
        $ChkPath = $PrepFileRootPath
        #$ChkPath = $def.PrepFileRootPath
        if (Test-Path -LiteralPath $ChkPath)
        {
            remove-item -LiteralPath $ChkPath -Recurse -Force
        }
        $ChkPath = $ConvFileRootPath
        #$ChkPath = $def.ConvFileRootPath
        if (Test-Path -LiteralPath $ChkPath)
        {
            remove-item -LiteralPath $ChkPath -Recurse -Force
        }
        $CopyMedia = 1
    }
}
#Adding dependent scripts:
Import-Module ".\CopyMediaFromNetwork2Local.psm1" -Force
Import-Module ".\PrepareMediaForDisplay.psm1" -Force
Import-Module ".\PackMediaIntoVideo.psm1" -Force

#Build derived definitions
$OutputDefs | Add-Member -MemberType NoteProperty -Name Outpath -Value $([string])
$OutputDefs | Add-Member -MemberType NoteProperty -Name OutGrp -Value $([string])
$OutputDefs | Add-Member -MemberType NoteProperty -Name VidPack -Value $([Int])
$DirSepChar = [System.IO.Path]::DirectorySeparatorChar
foreach($set in $OutputDefs)
{
    $Prepend  = [System.IO.Path]::GetFullPath($OutputFilePrepend, (Get-Location).Path)
    $Set.Outpath = ($Prepend+$set.XDim+"x"+$set.YDim+"q"+$set.Quality)
    $Set.OutGrp = ($Set.Outpath+"-Groups")
    if($Set.PicDispTime -and $Set.BulkVidTimeMin -and $Set.ImgVidFldr.Count)
    {
        $Set.VidPack = 1
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
    $PrepMediaDef = Update-ConvertedMediaImagesForDisplay $PrepFileRootPath $ConvFileRootPath
    Update-MediaForDisplaySets $PrepMediaDef $OutputDefs $SetTmpPath
}
if (($ProcLvl -eq 0) -or ($ProcLvl -eq 3)){
    Write-Host "***************************************"
    Write-Host "**** Building final output videos *****"
    Write-Host "***************************************"
    Set-VideoFromMedia $OutputDefs
}
$stopwatch.Stop()
$elapsedTime = $stopwatch.Elapsed
write-host "Elapsed time: $elapsedTime"
Read-Host -Prompt "Press enter to continue..."


