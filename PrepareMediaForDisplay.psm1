function New-VideoZoomedOutFromPic
{
    param (
        [string]$InputPicPath = "",
        [string]$ContPicPath = "",
        [Int]$InputWidth = 0,
        [Int]$InputHeight = 0,
        [decimal] $SrtZoom = 0,
        [decimal] $ZoomRate = 0,
        [decimal] $MaxRotAngl = 0,
        [decimal] $XRatio = 0,
        [decimal] $YRatio = 0,
        [decimal] $NFramesTrn = 0,
        [decimal] $NFramesStd = 0,
        [Int]$OutWidth = 0,
        [Int]$OutHeight = 0,
        [string]$FFMPEGCmdSrtAppend = "",
        [string]$FFMPEGCmdNomAppend = "",
        [string]$FFMPEGCmdEndAppend = "",
        [string]$SetTmpPath = ""
    )
    function HypDistance
    {
        param (
            $XDist,
            $YDist
        )
        return([Math]::Sqrt(`
    ([Math]::Pow(($XDist),2))+`
    ([Math]::Pow(($YDist),2))`
    ))
    }
    function ATan2Abs
    {
        param (
            [decimal] $XDist,
            [decimal] $YDist
        )
        return([Math]::Atan2( [Math]::abs($XDist), [Math]::abs($YDist)))
    }
    function AdjacentRadians
    {
        param (
            $Adj,
            $Hyp
        )
        return([Math]::acos([Math]::abs($Adj)/[Math]::abs($Hyp)))
    }
    function ComplRad
    {
        param (
            $AngleInRad
        )
        return([Math]::PI/2 - $AngleInRad)
    }

    function GetZoomedImgProps
    {
        param (
            [decimal] $IWidth, [decimal] $IHeight,
            [decimal] $Xratio, [decimal] $Yratio,
            [decimal] $SubImgOrigXRatio, [decimal] $SubImgOrigYRatio,
            [decimal] $SetZoom,  [decimal] $SetAngle,
            [decimal] $In2OutPxRatio, $OutCentX, $OutCentY
        )
        #Calculate offsets for the frame
        $RatioOfImage2Use  = (1.0/$SetZoom)
        $SrtImageBuffer = (1 - $RatioOfImage2Use) #Buffer region around image,
        #Coordinates to upper left corner of unrotated subimage
        $LeftDistToSubImg  = $XRatio*$SrtImageBuffer*$IWidth
        $TopDistToSubImg   = $YRatio*$SrtImageBuffer*$IHeight
        #Coordinates to subimage center (presentation anchor
        $SubImgWidth  =  $IWidth*$RatioOfImage2Use
        $SubImgHeight = $IHeight*$RatioOfImage2Use
        $SubImgTL2OrigX = $SubImgOrigXRatio*$SubImgWidth
        $SubImgTL2OrigY = $SubImgOrigYRatio*$SubImgHeight
        $SubImgTL2CenterX = 0.5*$SubImgWidth
        $SubImgTL2CenterY = 0.5*$SubImgHeight
        $SubImgCenterX = ($LeftDistToSubImg + $SubImgTL2CenterX)
        $SubImgCenterY = ($TopDistToSubImg + $SubImgTL2CenterY)
        $Draw2CentRatio = 0
        $SelCenterX = $SubImgCenterX + (($IWidth/2)-$SubImgCenterX)*$Draw2CentRatio
        $SelCenterY = $SubImgCenterY + (($IHeight/2)-$SubImgCenterY)*$Draw2CentRatio
        $SubImgOrig2CentX = $SelCenterX - ($LeftDistToSubImg + $SubImgTL2OrigX)
        $SubImgOrig2CentY = $SelCenterY - ($TopDistToSubImg + $SubImgTL2OrigY)
        $SubImgOrig2CentAngInRadians = [Math]::Atan2($SubImgOrig2CentY,$SubImgOrig2CentX)
        $SubImgOrig2CentDist = HypDistance $SubImgOrig2CentX $SubImgOrig2CentY
        $PostRotOrig2CenterAngInRadians = $SubImgOrig2CentAngInRadians + ([Math]::Pi/(180.0))*$SetAngle
        $PostScaleOrig2CentDist = $SubImgOrig2CentDist*$In2OutPxRatio*$SetZoom
        $PostScaleOrig2CentDistX = ([Math]::cos($PostRotOrig2CenterAngInRadians))*$PostScaleOrig2CentDist
        $PostScaleOrig2CentDistY = ([Math]::sin($PostRotOrig2CenterAngInRadians))*$PostScaleOrig2CentDist

        $retval = "" | Select-Object -Property InOrigX,InOrigY,SubImgTL2OrigX,SubImgTL2OrigY, `
        SubImgTLX,SubImgTLY,SubImgWidth,SubImgHeight, `
        OutCanvOrigX,OutCanvOrigY,OutCentX,OutCentY,InCentX,InCentY
        $retval.InOrigX = $LeftDistToSubImg + $SubImgTL2OrigX
        $retval.InOrigY = $TopDistToSubImg +  $SubImgTL2OrigY
        $retval.SubImgTL2OrigX = $SubImgTL2OrigX
        $retval.SubImgTL2OrigY = $SubImgTL2OrigY
        $retval.SubImgTLX    = $LeftDistToSubImg
        $retval.SubImgTLY    = $TopDistToSubImg
        $retval.SubImgWidth  = $SubImgWidth
        $retval.SubImgHeight = $SubImgHeight
        $retval.OutCanvOrigX = $OutCentX - $PostScaleOrig2CentDistX
        $retval.OutCanvOrigY = $OutCentY - $PostScaleOrig2CentDistY
        $retval.OutCentX = $OutCentX
        $retval.OutCentY = $OutCentY
        $retval.InCentX = $SelCenterX
        $retval.InCentY = $SelCenterY
        return $retval
    }

    $NFrames = $NFramesTrn*2 + $NFramesStd
    $Prescaler = 1 #Set to 0 to disable prescaling.  Will be ignored if less than 1 to prevent excess image definition.
    #Temp overrides
    #Constant related to deceleration rate:
    #Do not exceed 180, for monotonically increasing deceleration, do not exceed 90, general 30 - 45 is  smooth.
    $RotAngSlopeSrtAngInDeg = 45
    $RotAngSlopeSrtAng = ([Math]::PI/180)*$RotAngSlopeSrtAngInDeg
    $RotAngSlopeSrtX   = [Math]::cos($RotAngSlopeSrtAng)
    #Reference notes:
    # Clone ref:
    #   https://stackoverflow.com/questions/76961118/imagemagick-how-do-i-reuse-one-single-image-to-overlay-it-multiple-times
    #   https://stackoverflow.com/questions/29736137/imagemagick-multiple-operations-in-single-invocation
    #   https://stackoverflow.com/questions/73708237/can-imagemagick-generate-multiple-outputs-from-one-input

    # https://www.imagemagick.org/script/command-line-options.php#distort
    # https://im.snibgo.com/animsrt.htm
    if (Get-Command magick -ErrorAction SilentlyContinue) {}
    else {throw  "Image Magick not detected, images will not be converted"}
    if (Get-Command ffmpeg -ErrorAction SilentlyContinue) {}
    else {throw  "ffmpeg not detected, images will not be converted"}
    #Check temp directory:

    if($SetTmpPath.Length){
        $drive = (Get-Item $SetTmpPath).Root
        #Method 1
        #$disk = Get-WmiObject Win32_LogicalDisk | Where-Object {$_.DeviceID -eq $drive.Name.Trim("\")}
        #$remainingSpaceMB = ($disk.FreeSpace / 1MB)
        #Method 2
        $driveInfo = Get-PSDrive -Name $drive.Name[0]
        $remainingSpaceMB = $driveInfo.Free / 1MB

        if ($remainingSpaceMB -lt 1000)
        {
            Write-Host "Provided temp directory has less than 1 GB of space remaining, converter will attempt to use local user temp space."
            $SetTmpPath = $env:TEMP
        }
    }
    else{
        $SetTmpPath = $env:TEMP
    }

    #Define common command definitions
    if ($TestFldr)
    {
        $TmpDirName = $TestFldr
    }
    else
    {
        $TmpDirName =[System.IO.Path]::GetFileNameWithoutExtension($ContPicPath) + (Get-Date -Format "FileDateTime")
        $TmpDirName = $TmpDirName -replace "'", ""
    }
    $ErrLogPath = $SetTmpPath + "\" + $TmpDirName + "_ErrorLog.txt"
    try
    {
        #Get calculations for rotation based on image
        #If a max rotation is defined, randomize which direction we rotate from.
        if ($TestFldr)
        {
        } #Do nothing, rotation direction should be set per test config.
        else
        {
            if ($MaxRotAngl)
            {
                $RotDir = Get-Random -InputObject (-1, 1)
            }
            else
            {
                $RotDir = 0
            }
        }

        #Determine the border definitions required to meet the end output resolution.
        $whimgratio = $InputWidth/$InputHeight
        $whdispratio = $OutWidth/$OutHeight

        #Set border definitions, necessary so srt does not produce strange imaging artifacts.
        $bp = 1
        $borderdef = "$bp" + "X" + "$bp"
        $InputBorderDef = $borderdef

        #Calculate focus point based on selected rotation direction and selected ratios:
        $SrtRotAngle = 0
        if ($RotDir -eq 0)
        {
            $SubFocusRatioX = 0.5
            $SubFocusRatioY = 0.5
            $MaxAllowableRotationInRadians = 0
        }
        else
        {
            #If rotation is being used, calculate the focus and max angle applicable
            #If rotation is clockwise
            #$SrtZoom
            if ($RotDir -gt 0)
            {
                $SubFocusRatioX = 1 - $YRatio
                $SubFocusRatioY = $XRatio
            }
            #Else rotation is counter-clockwise
            else
            {
                $SubFocusRatioX = $YRatio
                $SubFocusRatioY = 1 - $XRatio
            }
        }

        if ($Prescaler -lt 1)
        {
            $Prescaler = 0
        }
        $PrescaleApply = 0
        if ($whimgratio -gt $whdispratio)
        {
            $PrescaledOrig2NewScale = $OutWidth/$InputWidth
            $CanvasHeight = $InputWidth/$whdispratio
            $CanvasWidth = $OutWidth
            $CanvasTopBotBorder = ($CanvasHeight - $InputHeight)/2
            $CanvasSideBorder = 0
            if ($Prescaler)
            {
                $PrescaleApply = ($OutWidth*$SrtZoom*$Prescaler)/($InputWidth)
            }
        }
        else
        {
            $PrescaledOrig2NewScale = $OutHeight/$InputHeight
            $CanvasWidth = $InputHeight*$whdispratio
            $CanvasHeight = $OutHeight
            $CanvasSideBorder = ($CanvasWidth - $InputWidth)/2
            $CanvasTopBotBorder = 0
            if ($Prescaler)
            {
                $PrescaleApply = ($OutHeight*$SrtZoom*$Prescaler)/($InputHeight)
            }
        }
        if (($PrescaleApply -lt 0.8) -and ($PrescaleApply -gt 0))
        {
            $PrescaleApplyPerc = $PrescaleApply*100
            $PrescaleCmd = " -adaptive-resize " + $PrescaleApplyPerc.ToString("00.00000") + "%"
            $InputWidth = $InputWidth*$PrescaleApply
            $InputHeight = $InputHeight*$PrescaleApply
            $Orig2NewScale = $PrescaledOrig2NewScale/$PrescaleApply
        }
        else
        {
            $PrescaleCmd = ""
            $Orig2NewScale = $PrescaledOrig2NewScale
        }

        $OutCentX = $OutWidth/2
        $OutCentY = $OutHeight/2


        $OutValSrt = GetZoomedImgProps $InputWidth $InputHeight $XRatio $YRatio `
        $SubFocusRatioX $SubFocusRatioY $SrtZoom 0 $Orig2NewScale $OutCentX $OutCentY

        #Write-Host "**********************************************"
        #Write-Host "*********** Wait Debugger Reached ************"
        #Write-Host "**********************************************"
        #Wait-Debugger

        #Get distance from subfocus origin to canvas boundaries (including border)
        $RestrictRotation2Content = 1
        if ($RestrictRotation2Content)
        {
            $TDist = $OutValSrt.InOrigY
            $LDist = $OutValSrt.InOrigX
            $BDist = $InputHeight - $OutValSrt.InOrigY
            $RDist = $InputWidth - $OutValSrt.InOrigX
        }
        else
        {
            $TDist = $OutValSrt.InOrigY + $CanvasTopBotBorder
            $LDist = $OutValSrt.InOrigX + $CanvasSideBorder
            $BDist = $InputHeight - $OutValSrt.InOrigY + $CanvasTopBotBorder
            $RDist = $InputWidth - $OutValSrt.InOrigX + $CanvasSideBorder
        }

        if ($RotDir -ne 0)
        {
            $SubImgLeftSide2OrigDist = $OutValSrt.SubImgTL2OrigX
            $SubImgTopSide2OrigDist = $OutValSrt.SubImgTL2OrigY
            $SubImgRightSide2OrigDist = $InputWidth*(1/$SrtZoom) - $OutValSrt.SubImgTL2OrigX
            $SubImgBotSide2OrigDist = $InputHeight*(1/$SrtZoom) - $OutValSrt.SubImgTL2OrigY

            #Get focus to corner distances in terms of full image, to be used to determine max rotation angle.
            $LTRadianAnglFromHorz = ATan2Abs $SubImgTopSide2OrigDist $SubImgLeftSide2OrigDist
            $LTCornerDist = HypDistance      $SubImgTopSide2OrigDist $SubImgLeftSide2OrigDist

            $RTRadianAnglFromHorz = ATan2Abs $SubImgTopSide2OrigDist $SubImgRightSide2OrigDist
            $RTCornerDist = HypDistance      $SubImgTopSide2OrigDist $SubImgRightSide2OrigDist

            $LBRadianAnglFromHorz = ATan2Abs $SubImgBotSide2OrigDist $SubImgLeftSide2OrigDist
            $LBCornerDist = HypDistance      $SubImgBotSide2OrigDist $SubImgLeftSide2OrigDist

            $RBRadianAnglFromHorz = ATan2Abs $SubImgBotSide2OrigDist $SubImgRightSide2OrigDist
            $RBCornerDist = HypDistance      $SubImgBotSide2OrigDist $SubImgRightSide2OrigDist
            #Figure out the max distance based on the angle of rotation:
            $TRotRadiansMax = [Math]::PI/2
            $LRotRadiansMax = [Math]::PI/2
            $BRotRadiansMax = [Math]::PI/2
            $RRotRadiansMax = [Math]::PI/2
            #Rotation is clockwise
            #Write-Host $SrtZoom.ToString()
            #Write-Host $XRatio.ToString()
            #Write-Host $YRatio.ToString()
            $TolChk = 1E-6
            if ($RotDir -gt 0)
            {
                if ($RTCornerDist -gt ($TDist + $TolChk))
                {
                    $TRotRadiansMax = (ComplRad $RTRadianAnglFromHorz) - (AdjacentRadians $TDist $RTCornerDist)
                }
                if ($LTCornerDist -gt ($LDist + $TolChk))
                {
                    $LRotRadiansMax = $LTRadianAnglFromHorz - (AdjacentRadians $LDist $LTCornerDist)
                }
                if ($LBCornerDist -gt ($BDist + $TolChk))
                {
                    $BRotRadiansMax = (ComplRad $LBRadianAnglFromHorz) - (AdjacentRadians $BDist $LBCornerDist)
                }
                if ($RBCornerDist -gt ($RDist + $TolChk))
                {
                    $RRotRadiansMax = $RBRadianAnglFromHorz - (AdjacentRadians $RDist $RBCornerDist)
                }
            }
            #Else rotation is counter-clockwise
            else
            {
                if ($RTCornerDist -gt ($RDist + $TolChk))
                {
                    $RRotRadiansMax = $RTRadianAnglFromHorz - (AdjacentRadians $RDist $RTCornerDist)
                }
                if ($LTCornerDist -gt ($TDist + $TolChk))
                {
                    $TRotRadiansMax = (ComplRad $LTRadianAnglFromHorz) - (AdjacentRadians $TDist $LTCornerDist)
                }
                if ($LBCornerDist -gt ($LDist + $TolChk))
                {
                    $LRotRadiansMax = $LBRadianAnglFromHorz - (AdjacentRadians $LDist $LBCornerDist)
                }
                if ($RBCornerDist -gt ($BDist + $TolChk))
                {
                    $BRotRadiansMax = (ComplRad $RBRadianAnglFromHorz) - (AdjacentRadians $BDist $RBCornerDist)
                }
            }
            $RotRadianArray = $TRotRadiansMax, $LRotRadiansMax, $BRotRadiansMax, $RRotRadiansMax, ($MaxRotAngl*([Math]::PI/180))
            $MaxAllowableRotationInRadians = ($RotRadianArray | Measure-Object -Minimum).Minimum
            if ($MaxAllowableRotationInRadians -lt 0)
            {
                $PsCmdlet.ThrowTerminatingError("WTF")
            }
            if ($MaxAllowableRotationInRadians)
            {
                $SetSrtRotInRad = Get-Random -Minimum ($MaxAllowableRotationInRadians/5) -Maximum $MaxAllowableRotationInRadians
            }
            else
            {
                $SetSrtRotInRad = 0
            }
            if ($RotDir -gt 0)
            {
                $SrtRotAngle = $SetSrtRotInRad*(180 / [Math]::PI)
            }
            else
            {
                $SrtRotAngle = $SetSrtRotInRad*(-180 / [Math]::PI)
            }
        }
        $BuildDir = $SetTmpPath + "\" + $TmpDirName
        $BorderImg = "`"" + $BuildDir + "\" + "refimg.jpg" + "`""
        $IMViewPortDef = "-define distort:viewport=$OutWidth" + "x" + "$OutHeight"
        $IMCmdSrt = "`"$( $file.ConvPath )`" " + $PrescaleCmd + " -bordercolor black -border $InputBorderDef -colorspace LAB -write MPR:orig -write $BorderImg -delete 0--1 $IMViewPortDef"
        $IMConvPrepend = "-read MPR:orig -distort SRT "
        $IMConvPreWrite = " -quality 92 -colorspace sRGB -write "
        $IMConvAppend = " -delete 0--1"
        $IMCmdEnd = ""
        if (-not (Test-Path $BuildDir -PathType Container))
        {
            (New-Item -Path $BuildDir -ItemType "directory") *> $null
        }

        $NFrames2StopRot = [Math]::ceiling(($NFramesTrn + $NFramesStd)*(2/3))
        $AtEndTransInd = $NFramesTrn + $NFramesStd
        $NFrameChars = [Math]::ceiling(([Math]::Log($NFrames)/[Math]::Log(10)))
        if ($NFrameChars -lt 1)
        {
            $NFrameChars = 1
        }
        $FDef = [string[]]::new($NFrameChars);
        $IMCmd = [string[]]::new($NFrames);
        $FFMPEGSrtVidInput = [string[]]::new($NFramesTrn);
        $FFMPEGNomVidInput = [string[]]::new($NFramesStd);
        $FFMPEGEndVidInput = [string[]]::new($NFramesTrn);
        for ($i = 0; $i -lt $NFrameChars; $i++) {
            $FDef[$i] = "0"
        }
        $FFmtDef = Join-String -InputObject $FDef
        $TPath = $BuildDir + "\" + "cmd2run.txt"
        $MgkPath = $BuildDir + "\" + "script.mgk"
        $SrtPath = $BuildDir + "\" + "SrtList.txt"
        $NomPath = $BuildDir + "\" + "NomList.txt"
        $EndPath = $BuildDir + "\" + "EndList.txt"
        for ($i = 0; $i -lt $NFrames; $i++) {
            $PreZoom = $SrtZoom - ($ZoomRate*$i)
            if ($PreZoom -lt 1.0)
            {
                $PreZoom = 1.0
            }

            $FPath = $BuildDir + "\" + $i.ToString($FFmtDef) + ".jpg"
            $RotChngInd = ($AtEndTransInd - $i)
            if ($RotChngInd -lt 0)
            {
                $RotChngInd = 0
                $SelRotAngl = 0
            }
            $SelSlopeAng = (($RotAngSlopeSrtAng*$RotChngInd)/$AtEndTransInd)
            $SelRotAngl = ((1 - [Math]::Cos($SelSlopeAng))/(1 - $RotAngSlopeSrtX))*$SrtRotAngle
            $OutValSet = GetZoomedImgProps $InputWidth $InputHeight $XRatio $YRatio `
                $SubFocusRatioX $SubFocusRatioY $PreZoom $SelRotAngl $Orig2NewScale $OutCentX $OutCentY
            #Define values for SRT to pass:
            $SetZoom = $PreZoom * $Orig2NewScale
            $SetRotate = $SelRotAngl
            $TxtFrmt = "00000.0000000000"
            $XOffsetInTxt = ($OutValSet.InOrigX + $bp).ToString($TxtFrmt)
            $YOffsetInTxt = ($OutValSet.InOrigY + $bp).ToString($TxtFrmt)
            $XOffsetOutTxt = ($OutValSet.OutCanvOrigX).ToString($TxtFrmt)
            $YOffsetOutTxt = ($OutValSet.OutCanvOrigY).ToString($TxtFrmt)
            $SetZoomTxt = $SetZoom.ToString($TxtFrmt)
            $SetRotateTxt = $SetRotate.ToString($TxtFrmt)
            #Add image file path to array, and add image magic command to array:
            $IMCmd[$i] = $IMConvPrepend + " $XOffsetInTxt,$YOffsetInTxt" + ",$SetZoomTxt,$SetRotateTxt,$XOffsetOutTxt,$YOffsetOutTxt "+ $IMConvPreWrite+ "`"$FPath`"" +"$IMConvAppend"
            #Ad d ffmpeg imporrt definition depending on where we're at
            $ImportStr = "file `'$FPath`'"
            if ($i -ge ($AtEndTransInd)){
                $FFMPEGEndVidInput[$i - $AtEndTransInd] = $ImportStr
            }
            elseif ($i -ge ($NFramesTrn))
            {
                $FFMPEGNomVidInput[$i - $NFramesTrn] = $ImportStr
            }
            else
            {
                $FFMPEGSrtVidInput[$i] = $ImportStr
            }
        }

        $ENLC = " ```r`n"
        $NLC  = "`r`n"

        #Now create command and run image magic to create the pictures.
        #if ($TestFldull)
        if ($TestFldr)
        {
        $MidFrame = [Math]::Ceiling($NFrames/2)
            $IMCmdMid = Join-String -InputObject $IMCmd[0, $MidFrame, -1] -Separator $NLC
        }
        else
        {
            $IMCmdMid = Join-String -InputObject $IMCmd -Separator $NLC
        }
        $IMCmdArray = $IMCmdSrt, $IMCmdMid, $IMCmdEnd
        $IMCmdRun = Join-String -InputObject $IMCmdArray -Separator $NLC
        $IMCmdRun | Out-File $MgkPath
        $AllCmds | Out-File $TPath

        #Now create the commands for ffmpeg for each video, and create the videos
        $FFMPEGPre = "ffmpeg -y -f lavfi -i anullsrc -f concat -safe 0 -i "

        $FFMPEGSettings = " "
        $FFMPEGSrtVidInputSet = Join-String -InputObject $FFMPEGSrtVidInput -Separator $NLC
        $FFMPEGSrtVidInputSet | Out-File $SrtPath
        $FFMPEGSrtVidArray = $FFMPEGPre, "`'", $SrtPath, "`'", $FFMPEGSettings, $FFMPEGCmdSrtAppend
        $FFSrtCmd = Join-String -InputObject $FFMPEGSrtVidArray -Separator $ENLC

        $FFMPEGNomVidInputSet = Join-String -InputObject $FFMPEGNomVidInput -Separator $NLC
        $FFMPEGNomVidInputSet | Out-File $NomPath
        $FFMPEGNomVidArray = $FFMPEGPre, "`'", $NomPath, "`'", $FFMPEGSettings, $FFMPEGCmdNomAppend
        $FFNomCmd = Join-String -InputObject $FFMPEGNomVidArray -Separator $ENLC

        $FFMPEGEndVidInputSet = Join-String -InputObject $FFMPEGEndVidInput -Separator $NLC
        $FFMPEGEndVidInputSet | Out-File $EndPath
        $FFMPEGEndVidArray = $FFMPEGPre, "`'", $EndPath, "`'", $FFMPEGSettings, $FFMPEGCmdEndAppend
        $FFEndCmd = Join-String -InputObject $FFMPEGEndVidArray -Separator $ENLC

        $AllCmdsSet = $IMCmdRun, $FFSrtCmd, $FFNomCmd, $FFEndCmd
        $AllCmds = Join-String -InputObject $AllCmdsSet -Separator "`r`n`r`n"


        #Preparee / Save all commands for debug if enabled
        $FFSrtCmdExe = $FFSrtCmd -replace $ENLC, ""
        $FFNomCmdExe = $FFNomCmd -replace $ENLC, ""
        $FFEndCmdExe = $FFEndCmd -replace $ENLC, ""
        #Perform all actions
        #Measure-Command { (magick -script $MgkPath) *> $null }
        (magick -script $MgkPath) *> $null
        (Invoke-Expression $FFSrtCmdExe) *> $null
        (Invoke-Expression $FFNomCmdExe) *> $null
        (Invoke-Expression $FFEndCmdExe) *> $null
        #Write-Host "Done"
        }
    catch{
        $errorMessage = $_.Exception.Message
        $errorDetails = $_.ErrorDetails
        $failedItem = $_.TargetObject
        $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
        if($SetRotate) {$RotRepTxt = "SetRotate: " + $SetRotate}
        else {$RotRepTxt = "SetRotate: Never Defined"}
        if($RotRadianArray) {$RadArryRepText = "RotRadianArray: " + $RotRadianArray}
        else {$RadArryRepText = "RotRadianArray: Never Defined"}
        if($OutValSrt) {$OutValSrtRepText = "OutValSrt: `r`n" + $OutValSrt}
        else {$OutValSrtRepText = "OutValSrt: Never Defined"}
        # Writing to a file
        $logEntry = "*****Failed conversion*****" + `
        "InputPicPath: " + $InputPicPath + "`r`n" + `
        $RotRepTxt + "`r`n" + `
        "IW: " + $image.Width + "`r`n" + `
        "IH: " + $image.Height + "`r`n" + `
        "SrtZoom: " + $SrtZoom + "`r`n" + `
        "ZoomRate: " + $ZoomRate + "`r`n" + `
        "MaxSrtRot: " + $MaxSrtRot + "`r`n" + `
        "XRatio: " + $XRatio + "`r`n" + `
        "YRatio: " + $YRatio + "`r`n" + `
        "NFramesTrn: " + $NFramesTrn + "`r`n" + `
        "NFramesStd: " + $NFramesStd + "`r`n" + `
        "XDim: " + $XDim + "`r`n" + `
        "YDim: " + $YDim + "`r`n" + `
        "YDim: " + $YDim + "`r`n" + `
        $OutValSrtRepText + "`r`n" + `
        $RadArryRepText + "`r`n" + `
        "$timestamp - Error: $errorMessage - Details: $errorDetails - Item: $failedItem" + `
                    " **************************** "
        Add-Content -Path $ErrLogPath -Value $logEntry
    }
    #Cleanup
    finally{
        (Remove-Item -LiteralPath $BuildDir -Recurse -Force -EA SilentlyContinue -Verbose)*>$null
    }
}

function Update-ConvertedMediaImagesForDisplay
{
    param (
        [string]$GenFrmt,
        [string]$PrepFileRootPath,
        [string]$ConvFileRootPath
    )
    if (Get-Command jpegr -ErrorAction SilentlyContinue) {$RotImg = 1}
    else {throw  "Jpeg lossless rotator not detected, images will not be automatically rotated, but this should be accomidated general conversion."}
    if (Get-Command magick -ErrorAction SilentlyContinue) {}
    else {throw  "Image Magick not detected, images will not be converted"}

    $HashTblDateFormat = "O"
    $CurrInnerProgPercInt = [int32[]]::new(1);
    $PrevInnerProgPercInt = [int32[]]::new(1);
    $InnerLoopProg = @{
        ID = 1
        Activity = "Getting ready.  Please wait..."
        Status = "Getting ready.  Please wait..."
        PercentComplete = 0
        CurrentOperation = 0
    }

    $ImgTypes = @("jpg", "gif", "tif", "tiff", "jpeg", "png", "bmp")
    $VidTypes = @("wmv", "mov", "mp4", "avi", "WEBM", "mkv")

    #************************************************************
    #******************Step 2, convert images.*******************
    #************************************************************
    $AllPrepFiles = @(Get-ChildItem -LiteralPath $PrepFileRootPath -Recurse -File) | Sort-Object Name
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name RelPath -Value $( [string] )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name ConvPath -Value $( [string]"" )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name ContExt -Value $( [string] )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name LastWriteTimeStr -Value $( [string] )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name SelLabelGrp -Value $( [string] )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name Need2ConvFlag -Value $( [int]0 )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name ConvExpected -Value $( [int]0 )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name IsImg -Value $( [int]0 )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name IsVid -Value $( [int]0 )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name FileIdx -Value $( [int]0 )
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name TupleVal -Value [System.ValueTuple[string, long, datetime]]
    $AllPrepFiles | Add-Member -MemberType NoteProperty -Name Exported -Value $( [int]0 )
    #If there are prep files, process.
    if ($AllPrepFiles.Count)
    {
        #*****************************************************
        #* Getting high level expectations for prep files ****
        #*****************************************************
        $PrpL = $PrepFileRootPath.Length
        $CurrentPrepFileTupleExists = @{ }
        if($ConvFileRootPath.Length)
        {
            $ConvFlgSet = 1
            $ConvReportPath = ($ConvFileRootPath + "\Report.csv")
            $ConvReportTupleExists = @{ }
        }
        else
        {
            $ConvFlgSet = 0
        }
        if ($ConvFlgSet -and (Test-Path -LiteralPath $ConvReportPath))
        {
            $PrevConvProps = Import-Csv -LiteralPath $ConvReportPath
            $PrevConvProps | Add-Member -MemberType NoteProperty -Name LastWriteTimeDateTime -Value $( [DateTime] )
            $PrevConvProps | Add-Member -MemberType NoteProperty -Name RemoveFlag -Value $( [int]0 )
            $PrevConvProps | Add-Member -MemberType NoteProperty -Name TupleVal -Value [System.ValueTuple[string, long, datetime]]
            Write-Host ($PrevConvProps.Count.ToString() + " files to check for previous properties")
            foreach ($PrevProp in $PrevConvProps)
            {
                $PrevProp.LastWriteTimeDateTime = [datetime]::ParseExact($PrevProp.LastWriteTimeStr, $HashTblDateFormat, $null)
                $key = [System.ValueTuple[string, long, datetime]]::new(
                        $PrevProp.RelPath, $PrevProp.Length, $PrevProp.LastWriteTimeDateTime)
                #If the file actually exists, set the flag so that it's not converted again.
                if(Test-Path $PrevProp.ConvPath -PathType Leaf)
                {
                    $PrevProp.TupleVal = $key
                    $ConvReportTupleExists[$key] = 1
                }
            }
        }
        Write-Host ($AllPrepFiles.Count.ToString() + " files to get attributes for...")
        $FileCntr = 0
        foreach ($file in $AllPrepFiles)
        {
            $FileCntr++
            $file.FileIdx = $FileCntr
            $ExtLen = $file.FullName.Length - $PrpL
            $RelPath = $file.FullName.Substring($PrpL, $ExtLen)
            $file.RelPath = $RelPath
            $file.LastWriteTimeStr = $file.LastWriteTime.ToString($HashTblDateFormat)
            $datekey = [System.ValueTuple[string, long, datetime]]::new(
                    $RelPath, $file.Length, $file.LastWriteTime)
            #Set tuple for current conversion map, for removal reference.
            $CurrentPrepFileTupleExists[$datekey] = 1
            $file.TupleVal = $datekey
            $IncChk = 0
            foreach ($type in $ImgTypes)
            {
                if ( $file.Name.EndsWith($type))
                {
                    if($ConvFlgSet){$file.ConvExpected = 1}
                    $file.IsImg = 1
                    $file.ContExt = ".jpg"
                    $IncChk = 1
                }
            }
            foreach ($type in $VidTypes)
            {
                if ( $file.Name.EndsWith($type))
                {
                    $file.IsVid = 1
                    $file.ContExt = ".$GenFrmt"
                }
            }
            #If it is an image, set the conversion source path accordingly.
            if ($IncChk)
            {
                if($ConvFlgSet)
                {
                    $file.ConvPath = Join-Path -Path $ConvFileRootPath -ChildPath $RelPath
                    if ($ConvReportTupleExists[$datekey])
                    {
                    }#do nothing, file exists and is already converted according to the conversion report.
                    elseif($IncChk)
                    {
                        $file.Need2ConvFlag = 1
                    }
                }
                else
                {$file.ConvPath = $file.FullName
                }
            }
        }
        #Remove items that shouldn't be there.  Not really necessary but good to cleanup
        if($ConvFlgSet)
        {
            Write-Host ("Checking for old converted files to remove")
            foreach ($PrevProp in $PrevConvProps)
            {
                if ($CurrentPrepFileTupleExists[$PrevProp.TupleVal])
                {
                } #Do nothing if file should exist.
                #elseif(Test-Path -LiteralPath $PrevProp.ConvPath){} #Do nothing if there is no path information.
                elseif(Test-Path -Path $PrevProp.ConvPath -PathType Leaf)
                {
                    $PrevProp.RemoveFlag = 1
                }
            }
            $OldPrepFiles2Rem = ($PrevConvProps | Where-Object -Property RemoveFlag -eq 1)
            Write-Host ($OldPrepFiles2Rem.Count.ToString() + " old converted media files to remove!")
            foreach ($PrevProp in $OldPrepFiles2Rem)
            {
                remove-item -LiteralPath $PrevProp.ConvPath -Force
            }
            $Files2Conv = ($AllPrepFiles | Where-Object -Property Need2ConvFlag -eq 1)
            Write-Host ($Files2Conv.Count.ToString() + " media files to convert!")
            $PrevInnerProgPercInt[0] = -1
            $LoopProg = 0
            $AllFilesizeTtl = $Files2Conv | Measure-Object -Property Length -Sum; $AllFilesizeTtl = $AllFilesizeTtl.Sum
            if ($Files2Conv.Count)
            {
                $ConvDirs2Batch = (Split-Path $Files2Conv.ConvPath -Parent) | Get-Unique | Sort-Object { $_.Length }
            }
            foreach ($file in $Files2Conv)
            {
                if ( -not(Test-Path -LiteralPath $file.ConvPath -PathType Leaf))
                {
                    #Create file template
                    $null = New-Item -ItemType File -Path $file.ConvPath -Force
                }
                #Do the conversion stuff here
                copy-item $file.FullName $file.ConvPath
                #if($RotImg){$null = jpegr $file.ConvPath}
                #if($MagImg){$null = magick mogrify -autocolor -autotone -enrich -autogamma $file.ConvPath}
                $file.Exported = 1
                $LoopProg += $file.Length
                $CurrInnerProgPercInt[0] = ($LoopProg*100)/$AllFilesizeTtl
                if ($CurrInnerProgPercInt[0] -gt $PrevInnerProgPercInt[0])
                {
                    $InnerLoopProg.PercentComplete = $CurrInnerProgPercInt[0]
                    $PrevInnerProgPercInt[0] = $CurrInnerProgPercInt[0]
                    $InnerLoopProg.Status = "Converting files: " + $InnerLoopProg.PercentComplete.ToString() + "% Complete"
                    Write-Progress @InnerLoopProg
                }
            }
            Write-Progress @InnerLoopProg -Completed
            if ($RotImg -and $ConvDirs2Batch.Count)
            {
                Write-Host ($ConvDirs2Batch.Count.ToString() + " media folders to batch rotate!")
                $PrevInnerProgPercInt[0] = 0
                $LoopProg = 0
                $AllFilesizeTtl = $ConvDirs2Batch.Count
                foreach ($fldr in $ConvDirs2Batch)
                {
                    $jpegrcmd = "jpegr -auto -s `"$fldr`""
                    (Invoke-Expression $jpegrcmd) *> $null
                    $LoopProg ++
                    $CurrInnerProgPercInt[0] = ($LoopProg*100)/$AllFilesizeTtl
                    if ($CurrInnerProgPercInt[0] -gt $PrevInnerProgPercInt[0])
                    {
                        $InnerLoopProg.PercentComplete = $CurrInnerProgPercInt[0]
                        $PrevInnerProgPercInt[0] = $CurrInnerProgPercInt[0]
                        $InnerLoopProg.Status = "Rotating Images: " + $InnerLoopProg.PercentComplete.ToString() + "% Complete"
                        Write-Progress @InnerLoopProg
                    }
                }
                Write-Progress @InnerLoopProg -Completed

            }
            $FilesConverted = @(($AllPrepFiles | Where-Object -Property ConvExpected -eq 1) | Where-Object -Property Need2ConvFlag -eq 0)
            if ($FilesConverted.Count){
                $FilesConverted = $FilesConverted + @($Files2Conv | Where-Object -Property Exported -eq 1)}
            else {$FilesConverted = @($Files2Conv | Where-Object -Property Exported -eq 1)}
            $FilesConverted | Select-Object -Property Name,RelPath,ConvPath,Length,LastWriteTimeStr|
                    Export-Csv -LiteralPath $ConvReportPath -NoTypeInformation

        }
    }
    Write-Output $AllPrepFiles
}


function Update-MediaForDisplaySets
{
    param (
        [string]$GenFrmt,
        $AllPrepFiles,
        $OutputSizes,
        [string]$SetTmpPath = ""
    )
    if (Get-Command ffmpeg -ErrorAction SilentlyContinue)
    {
    }
    else
    {
        throw  "ffmpeg not detected, videos will not be converted"
    }
    if (Get-Command magick -ErrorAction SilentlyContinue)
    {
    }
    else
    {
        throw  "Image Magick not detected, videos will not be converted"
    }

    #If the temp path doesn't exist, set it to the user temp space.
    if($SetTmpPath.Length)
    {
        #If it's valid, keep it as is, else change it to the user temp space.
        if (Test-Path -Path $SetTmpPath)
        {
        }
        else{
            $SetTmpPath = $env:TEMP
        }
    }
    else{
        $SetTmpPath = $env:TEMP
    }

    $CurrInnerProgPercInt = [int32[]]::new(1);
    $PrevInnerProgPercInt = [int32[]]::new(1);
    $HashTblDateFormat = "O"
    $arval = 48000 #audio rate
    $vrate = 60000 #timescale
    $tqual = 10 #Quality to convert to for transitions, higher quality because these must be reencoded with fade to reduce loss.
    $GDefs = @{
        MaxSrtZoom = 1.5
        MinSrtZoom = 1.2
        frameRate = 0 #Configured below per set.
        audiorate = $arval
        videorate = $vrate
        tq = $tqual
        vq = $tqual #Is updated per set definition.
        ffmpegvcdcstd = "" #Configured below per set.
        ffmpegvcdctra = "" #Configured below per set.
        ffmpegaudcmd = "-c:a aac -ar $arval -shortest "
    }
    $ExpFileTupleNonZeroIdx = @{ }
    $ExpFileRelPathExists = @{ }
    #Now, for each set, run the final export tooling depending on if the file is a video or image.
    #********************************************************************************************
    #********************************************************************************************
    #********************************************************************************************
    $AllFiles = @($AllPrepFiles | Where-Object {($_.IsImg -eq 1) -or ($_.IsVid -eq 1)})
    #Define file existance and up-to-date definitions.
    Write-Host ($AllFiles.Count.ToString() + " media files to prepare for content presentation!")
    $idx = 0
    foreach ($file in $AllFiles)
    {
        $idx++
        $ExpFileTupleNonZeroIdx[$file.TupleVal] = $idx
        $ExpFileRelPathExists[$file.relpath] = 1
    }
    $CurrDateTime = Get-Date
    foreach ($set in $OutputSizes)
    {
        #Get group names for files according to set definition.
        #Definitions for exporting, which will be used in actual data export.
        $GDefs.frameRate = $set.FPS
        $GDefs.MaxSrtRot = $set.MaxSrtRot
        $GDefs.ffmpegvcdcstd = "-video_track_timescale $vrate -framerate $($Set.fps ) -vcodec libx264 -crf $($Set.Quality ) -colorspace 1 -preset slow -pix_fmt yuvj420p -r $( $set.FPS ) -movflags faststart "
        $GDefs.ffmpegvcdctra = "-video_track_timescale $vrate -framerate $($Set.fps ) -vcodec libx264 -crf $($GDefs.tq ) -colorspace 1 -preset slow -pix_fmt yuvj420p -r $( $set.FPS ) -movflags faststart "
        #Clear-Variable -Name "Files2Chk"
        $Files2Chk = $AllFiles
        $Files2Chk | Add-Member -MemberType NoteProperty -Name RelContPath -Value $( [string]"" ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name ContCreationDate -Value $( [datetime] ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name ContCreationDateStr -Value $( [string] "" ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name ContPath -Value $( [string]"" ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name ContTitle -Value $( [string]"" ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name ImgVidPFlg -Value $( [int]0 ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name RelImgVidPath -Value $( [string]"" ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name ImgVidPath -Value $( [string]"" ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name InstInd -Value $( [int]0 ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name Exp2ContPath -Value $( [int]0 ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name ExpDefComplete -Value $( [int]0 ) -Force
        $Files2Chk | Add-Member -MemberType NoteProperty -Name PreRepExpIndP1 -Value $( [int]0 ) -Force
        foreach ($file in $Files2Chk)
        {
            if ($file.IsImg -and $set.ImgVidFldr.Length -and $set.PicDispTime)
            {
                $file.ImgVidPFlg = 1
            }
            $TenativeLbl = "File"
            if ( $set.NameMethod.StartsWith("FldrLvl"))
            {
                $LvlIdx = [Int]$set.NameMethod.split("FldrLvl")[1]
                $Parts = $file.relpath -split '\\'
                #If the level index desired is folder, use it, else keep the label designation blank.
                if ($Parts.Count -ge ($LvlIdx + 1))
                {
                    $TenativeLbl = $Parts[$LvlIdx]
                }
            }
            else
            {
            }
            if ($TenativeLbl.Length)
            {
                $CleanedLabel = $TenativeLbl -replace "[^a-zA-Z0-9 _-]"
                $file.SelLabelGrp = $CleanedLabel
            }
            else
            {
                $file.SelLabelGrp = "File"
            }
        }
        #Create common definitions for set.
        Write-Host ("Exporting media for set: " + $set.XDim + " by " + $set.YDim)
        $ContFileRootPath = $set.Outpath
        $VidPacksRootPath = $ContFileRootPath + $set.ImgVidFldr
        if (-not (Test-Path -LiteralPath $ContFileRootPath -PathType Container))
        {
            New-Item -Path $ContFileRootPath -ItemType "directory" | Out-Null
        }
        if ($set.ImgVidFldr.length -and $set.PicDispTime -and (-not (Test-Path -LiteralPath $VidPacksRootPath -PathType Container)))
        {
            New-Item -Path $VidPacksRootPath -ItemType "directory" | Out-Null
        }
        $ContReportPrePath = ($ContFileRootPath + "\PreReport.csv")
        #Define files groups that should exxist so they are not reomved.
        #$PrevContInd = @{ }
        $PrevFileSet = @{ }
        #$selfilename = Split-Path -Path $ContReportPrePath -Leaf
        $PrevFileSet[$ContReportPrePath] = 1
        #Remove old content items if they are not up to date anymore.
        Write-Host ("Checking report file to verify integrity and determine which files need to be updated...")
        if (Test-Path -Path $ContReportPrePath)
        {
            $PrevContProps = Import-Csv -LiteralPath $ContReportPrePath
            $PrevContProps| Add-Member -MemberType NoteProperty -Name ContCreationDate -Value $([DateTime])  -Force
            Write-Host ("Checking " + $PrevContProps.Count.ToString() + " old entries...")
            $RepIdxP1 = 0
            foreach ($SelProp in $PrevContProps)
            {
                $fileU2D = 0
                $RepIdxP1++
                #Check to see if all properties in the report are up-to-date.
                $DateTimeVal = [datetime]::ParseExact($SelProp.LastWriteTimeStr, $HashTblDateFormat, $null)
                $datekey = [System.ValueTuple[string, long, datetime]]::new(
                        $SelProp.RelPath, $SelProp.Length, $DateTimeVal)

                if($SelProp.RelContPath.Endswith($GenFrmt)){

                #write-host($SelProp.RelContPath)
                }
                if ($SelProp.RelContPath.Length)
                {
                    #If the file exists, check the creation date
                    $FullContPath = Join-Path -Path $ContFileRootPath -ChildPath $SelProp.RelContPath
                    if (Test-Path $FullContPath -PathType Leaf)
                    {
                        $SelCreationTime = [datetime]::ParseExact($SelProp.ContCreationDateStr, $HashTblDateFormat, $null)
                        $SelProp.ContCreationDate = $SelCreationTime
                        #If the creation date matches, check to see if the video image is up-to-date
                        if ($SelCreationTime -eq (Get-Item -LiteralPath "$FullContPath").CreationTime)
                        {
                            #See if a corresponding file index exists
                            if ($ExpFileTupleNonZeroIdx[$datekey])
                            {
                                #See if there should be a corresponding image path.
                                if ($Files2Chk[$ExpFileTupleNonZeroIdx[$datekey]-1].ImgVidPFlg)
                                {
                                    if ($SelProp.RelImgVidPath.Length)
                                    {
                                        $FullImgPath = $ContFileRootPath + $SelProp.RelImgVidPath
                                        if (Test-Path $FullImgPath -PathType Leaf)
                                        {
                                            #If the creation date matches, check to see if the video image is up-to-date
                                            if ($SelCreationTime = (Get-Item -LiteralPath "$FullImgPath").CreationTime)
                                            {
                                                #If the creation date matches, check to see if the video image is up-to-date
                                                if ($SelCreationTime = (Get-Item -LiteralPath "$FullImgPath").CreationTime)
                                                {
                                                    $fileU2D = 1
                                                    $PrevFileSet[$FullImgPath] = 1
                                                }
                                            }
                                        }
                                    }

                                }
                                #If the files exists, but there should not be an image video, claim this index.
                                else
                                {
                                    $fileU2D = 1
                                }
                            }
                        }
                    }
                }
                #Set the index and group index for reference.
                if ($fileU2D)
                {
                    $Files2Chk[$ExpFileTupleNonZeroIdx[$datekey]-1].PreRepExpIndP1 = $RepIdxP1
                    $Files2Chk[$ExpFileTupleNonZeroIdx[$datekey]-1].InstInd = [Int] $SelProp.InstInd
                    $PrevFileSet[$FullContPath] = 1
                }
            }
        }
        #Remove files from the folder which didn't belong to the database.
        $AllCurrContFiles = @(Get-ChildItem -LiteralPath $ContFileRootPath -Recurse -File)
        $AllCurrContFiles | Add-Member -MemberType NoteProperty -Name RemFlag -Value $( [int]0 )  -Force
        foreach ($ContFile in $AllCurrContFiles)
        {
            #If this is a transition file and the core file exists, assume all three can stay (don't tag for removal)
            $PathLen = $ContFile.FullName.Length
            if ($ContFile.Fullname.EndsWith("srt."+$GenFrmt) -or $ContFile.Fullname.EndsWith("end."+$GenFrmt))
            {
                $CoreName = $ContFile.Fullname.Substring(0, ($PathLen - (4+$GenFrmt.Length)))+$GenFrmt
            }
            else
            {
                $CoreName = $ContFile.Fullname
            }
            if ($PrevFileSet[$CoreName])
            {
            }#If file should exist, do nothing.
            else
            {
                $ContFile.RemFlag = 1
            }
        }
        $CurrContFiles2Rem = @($AllCurrContFiles| Where-Object -Property RemFlag -eq 1)
        Write-Host ("Removing " + $CurrContFiles2Rem.Count.ToString() + " file(s) that were not expected or out of date...")
        foreach ($ContFile in $CurrContFiles2Rem)
        {
            remove-item -LiteralPath $ContFile.FullName -Force
        }
        #Get index of current content items

        #Group files
        $FileGroups = @($Files2Chk | Group-Object -Property SelLabelGrp)
        Write-Host ("Checking " + $FileGroups.Count.ToString() + " group(s)...")
        foreach ($grp in $FileGroups)
        {
            $InstIdxSet = @{ }
            foreach ($file in ($grp| Select-Object -ExpandProperty Group))
            {
                #If a previous index already exists for this file, store it.
                if ($file.InstInd)
                {
                    $InstIdxSet[$file.InstInd] = 1
                }
            }
            #Populate any missing indexes for the group
            $GrpIdx = 1;
            foreach ($file in ($grp| Select-Object -Expand Group))
            {
                #Also set all the corresponding information for that index.
                if ($file.InstInd)
                {
                    #It's already been exported, so set the flag.
                    $file.ExpDefComplete = 1
                    $file.ContCreationDate = $PrevContProps[$file.PreRepExpIndP1-1].ContCreationDate
                    $file.RelContPath      = $PrevContProps[$file.PreRepExpIndP1-1].RelContPath
                    $file.RelImgVidPath    = $PrevContProps[$file.PreRepExpIndP1-1].RelImgVidPath
                }
                else
                {
                    #Incriment index till one is found that is not used.
                    if ($InstIdxSet.Count)
                    {
                        while ($InstIdxSet[$GrpIdx])
                        {
                            $GrpIdx++
                        }
                    }
                    #Once it's found, set it and set the map to indicate the index has been used.
                    $file.InstInd = $GrpIdx
                    $file.ContCreationDate = $CurrDateTime
                    $InstIdxSet[$GrpIdx] = 1
                    #Set the flag to export the content, since it's new.
                    $file.Exp2ContPath = 1
                }
                #Finally, convert datestr.
                $file.ContCreationDateStr = $file.ContCreationDate.ToString($HashTblDateFormat)

            }
        }
        #Ungroup all files back to the original variable.
        $Files2Chk = ($FileGroups| Select-Object -Expand Group)

        #Create the export path and perform the export.
        Write-Host ("Checking " + ($Files2Chk | Where-Object -Property Exp2ContPath -eq 1).Count.ToString() + " for content definitions...")
        foreach ($file in @($Files2Chk | Where-Object -Property Exp2ContPath -eq 1))
        {
            if ($file.SelLabelGrp.Length)
            {
                $Designator = $file.SelLabelGrp + "-" + $file.InstInd.ToString('00000')}
            else{ $Designator = $file.InstInd.ToString('00000') }
            $file.RelContPath = $Designator + $file.ContExt
            $file.ContPath = ($ContFileRootPath + "\" + $file.RelContPath)
            $file.ContTitle = $Designator
            #If the intent is also to convert the picture to a video, also define the path of the video to export to.
            try
            {
                if ($file.IsImg -and $set.ImgVidFldr.Length -and $set.PicDispTime)
                {
                    $file.RelImgVidPath = ($set.ImgVidFldr + "\" + $Designator + ".$GenFrmt")
                    $file.ImgVidPath = ($ContFileRootPath + $file.RelImgVidPath)
                }
                #It's already been exported, so set the flag.
                $file.ExpDefComplete = 1
            }
            catch
            {
            }
        }
        #Create report placeholder if it doesn't exist
        if (Test-Path -Path $ContReportPrePath)
        {
        }
        else
        {
            $null = New-Item -ItemType File -Path $ContReportPrePath -Force
        }
        ($Files2Chk | Where-Object -Property ExpDefComplete -eq 1) | Select-Object -Property RelPath,Length,LastWriteTimeStr,RelContPath,RelImgVidPath,ContCreationDateStr,InstInd|
                Export-Csv -LiteralPath $ContReportPrePath -NoTypeInformation
        #Save the prep file

        #Process the files

        $PrevInnerProgPercInt[0] = 0
        $LoopProg = 0
        $ShowProg = 0
        $RunSeries = 1
        $funcDef = ${function:New-VideoZoomedOutFromPic}.ToString()
        $Files2Chk | Add-Member -MemberType NoteProperty -Name ExportStr -Value $( [string]"") -Force
        $AllFilesizeTtl = ($Files2Chk| Where-Object -Property Exp2ContPath -eq 1) | Measure-Object -Property Length -Sum; $AllFilesizeTtl = $AllFilesizeTtl.Sum
        $ExpInd = 0
        $TotalNFiles2Exp = ($Files2Chk | Where-Object -Property Exp2ContPath -eq 1).Count.ToString()
        (($Files2Chk| Where-Object -Property Exp2ContPath -eq 1)) | ForEach-Object{
            $ExpInd++
            $_.ExportStr = $ExpInd.ToString() + " of " + $TotalNFiles2Exp
        }
        Write-Host ("Exporting " + ($Files2Chk | Where-Object -Property Exp2ContPath -eq 1).Count.ToString() + " files...")
        #Wait-Debugger
        (($Files2Chk| Where-Object -Property Exp2ContPath -eq 1)) | ForEach-Object -Parallel{
        #(($Files2Chk| Where-Object -Property Exp2ContPath -eq 1)) | ForEach-Object{
            if ($RunSeries) {
                $file = $_
                $XDim = $set.XDim
                $YDim = $set.YDim
                $FadeTime = $set.FadeTime
                $PicDispTime = $set.PicDispTime
                $VidPack = $set.VidPack
                $framerate =     $GDefs.framerate
                $MaxSrtRot =     $GDefs.MaxSrtRot
                $MinSrtZoom =    $GDefs.MinSrtZoom
                $MaxSrtZoom =    $GDefs.MaxSrtZoom
                $ffmpegvcdcstd = $GDefs.ffmpegvcdcstd
                $ffmpegvcdctra = $GDefs.ffmpegvcdctra
                $ffmpegaudcmd =  $GDefs.ffmpegaudcmd
                $VidRateTimescale =  $GDefs.videorate
                $CurrDateTime =  $CurrDateTime}
            else{
                ${function:New-VideoZoomedOutFromPic} = $using:funcDef
                $SetTmpPath                           = $using:SetTmpPath
                $file = $_
                $GenFrmt = $using:GenFrmt
                $XDim = $using:set.XDim
                $YDim = $using:set.YDim
                $FadeTime = $using:set.FadeTime
                $PicDispTime = $using:set.PicDispTime
                $VidPack = $using:set.VidPack
                $framerate = $using:GDefs.framerate
                $MaxSrtRot = $using:GDefs.MaxSrtRot
                $MinSrtZoom = $using:GDefs.MinSrtZoom
                $MaxSrtZoom = $using:GDefs.MaxSrtZoom
                $ffmpegvcdcstd = $using:GDefs.ffmpegvcdcstd
                $ffmpegvcdctra = $using:GDefs.ffmpegvcdctra
                $ffmpegaudcmd = $using:GDefs.ffmpegaudcmd
                $VidRateTimescale = $using:GDefs.videorate
                $CurrDateTime = $using:CurrDateTime}
            $ContDateTime = $file.ContCreationDate
            $whdispratio = $XDim/$YDim
            $Outstr = "Building content for file "+$file.ExportStr+" - $( $file.Name )..."
            write-host $Outstr
            #write-host "Codec export definition: $ffmpegvcdcstd"
            #Create common definitions.
            $SelFadeFrames = [Int]($FadeTime*$framerate)
            $SelFadeTime = ($SelFadeFrames/$framerate)
            $SelNormFrames = [Int]($PicDispTime*$framerate)
            $SelNormTime = ($SelNormFrames/$framerate)
            $frmtdef = $GenFrmt
            try
            {

                if ($file.IsImg)
                {
                    $filename = [System.IO.Path]::GetFileNameWithoutExtension($file.ImgVidPath)
                    $parentPath = Split-Path $file.ImgVidPath -Parent
                }
                else
                {
                    $filename = [System.IO.Path]::GetFileNameWithoutExtension($file.ContPath)
                    $parentPath = Split-Path $file.ContPath -Parent
                }
                $dirpath = Join-Path $parentPath $filename
                $srtpath = $dirpath+".srt.$GenFrmt"
                $nompath = $dirpath+".$GenFrmt"
                $endpath = $dirpath+".end.$GenFrmt"
                if ($file.IsImg)
                {
                    $ScaleWIM = 1
                    $image = New-Object -ComObject Wia.ImageFile
                    $image.loadfile($file.ConvPath)
                    $whimgratio = $image.Width/$image.Height
                    #If we're converting the picture to an image, it must oversized substantially to
                    #allow smooth zooming.  Keeping a whole number in case it is rendered to the nominal dimensions.
                    if (($file.ImgVidPath.length) -and ($ScaleWIM -ne 1))
                    {
                        $contw = [math]::Ceiling($XDim*4*$MaxSrtZoom)
                        $conth = [math]::Ceiling($YDim*4*$MaxSrtZoom)
                    }
                    #If width is greater, limit this dimension for resize.
                    elseif ($whimgratio -gt $whdispratio)
                    {
                        $contw = $XDim
                        $conth = ($XDim/$whimgratio)
                    }
                    else
                    {
                        $conth = $YDim
                        $contw = ($YDim*$whimgratio)
                    }
                    $wint = $contw -as [Int]
                    $hint = $conth -as [Int]
                    $SizeStr = $wint.ToString() + "x" + $hint.ToString()
                    $SizeStr2 = $XDim.ToString() + ":" + $YDim.ToString()
                    $SizeOut = $XDim.ToString() + "x" + $YDim.ToString()
                    $quality = 95
                    if ($file.ImgVidPath.length)
                    {
                        $ExpCmd = "-compose Copy -gravity center -extent  $SizeStr -quality $quality "
                        $PstCmd = "-compose Copy -gravity center -extent  $SizeOut -quality $quality "
                    }
                    else
                    {
                        $ExpCmd = ""
                        $PstCmd = ""
                    }
                    #magick input.jpg -resize 800x600 -background black -compose Copy \
                    #-gravity center -extent 800x600 -quality 92 output.jpg#Optional / future explore:
                    #$null = magick $file.ConvPath -auto-gamma -auto-level -white-balance -resize ($contw.ToString()+"x"+$conth.ToString()+">") $file.ContPath
                    #
                    $IMCmd1 = "magick `"$( $file.ConvPath )`" -auto-orient -resize $SizeStr -quality $($quality.ToString() ) -background black "
                    $IMCmdOut = "`"$( $file.ContPath )`""
                    $IMCmd = $IMCmd1 + $ExpCmd + $IMCmdOut
                    (Invoke-Expression $IMCmd) *> $null
                    [System.IO.File]::SetCreationTime( "$( $file.ContPath )", $CurrDateTime)
                    [System.IO.File]::SetCreationTime($file.ContPath, $CurrDateTime)
                    if ($file.ImgVidPath.length)
                    {
                        #Write-Host("**************************L1****************************")
                        $FullImgDur = $FadeTime*2 + $PicDispTime
                        $NFramesTrn = ($FadeTime*$FrameRate) -as [Int]
                        $NFramesStd = ($PicDispTime*$FrameRate) -as [Int]
                        $NFramesExp = ($FullImgDur*$FrameRate) -as [Int]
                        #Zoompan configuration here.
                        $SetSrtZoom = Get-Random -Minimum $MinSrtZoom -Maximum $MaxSrtZoom
                        $XRatio = Get-Random -Minimum 0.0 -Maximum 1.0
                        $YRatio = Get-Random -Minimum 0.0 -Maximum 1.0
                        $ZoomRate = ($SetSrtZoom - 1)/$NFramesExp
                        $SetNomZoom = $SetSrtZoom - ($ZoomRate*$SelFadeFrames)
                        $SetEndZoom = $SetNomZoom - ($ZoomRate*$SelNormFrames)

                        #Write-Host("**************************L2****************************")
                        $ffmpegCmd1 = "ffmpeg -y "
                        $ffmpegCmdA = "-f lavfi -i anullsrc  -loop 1 -f image2 "
                        $ffmpegCmdV1 = "-framerate " + $frameRate + " -i `"$( $file.ContPath )`" "
                        $SrtffmpegCmdV2 = "-t $SelFadeTime "
                        $Srtfiltercfg1 = "-filter_complex `"[1:v]zoompan=z='if(gte(in,1),min(pzoom-$ZoomRate,1.5),$SetSrtZoom)'"
                        $ffmpegCmdV2 = "-t $SelNormTime "
                        $filtercfg1 = "-filter_complex `"[1:v]zoompan=z='if(gte(in,1),min(pzoom-$ZoomRate,1.5),$SetNomZoom)'"
                        $EndffmpegCmdV2 = "-t $SelFadeTime "
                        $Endfiltercfg1 = "-filter_complex `"[1:v]zoompan=z='if(gte(in,1),min(pzoom-$ZoomRate,1.5),$SetEndZoom)'"
                        $filtercfgX = ":x='($wint*$XRatio*(1.0-1/zoom))'"
                        $filtercfgY = ":y='$hint*$YRatio*(1.0-1/zoom)'"
                        $filtercfg2 = ":d=1:fps=$frameRate`:s=$SizeOut`" "

                        #Write-Host("**************************L3****************************")
                        $ffmpegOutSrt = "-map 0:a -map 1:v -s $SizeStr2 `"$srtpath`""
                        $ffmpegOutNom = "-map 0:a -map 1:v -s $SizeStr2 `"$nompath`""
                        $ffmpegOutEnd = "-map 0:a -map 1:v -s $SizeStr2 `"$endpath`""
                        $ffmpegOutSrtIM = "-map 0:a -map 1:v -frames:v $NFramesTrn `"$srtpath`""
                        $ffmpegOutNomIM = "-map 0:a -map 1:v -frames:v $NFramesStd `"$nompath`""
                        $ffmpegOutEndIM = "-map 0:a -map 1:v -frames:v $NFramesTrn `"$endpath`""
                        #Write-Host("**************************L4****************************")
                        $ffmpegCmdSrt = $ffmpegCmd1 + $ffmpegCmdA + $ffmpegCmdV1 + $SrtffmpegCmdV2 `
                        + $Srtfiltercfg1 + $filtercfgX + $filtercfgY + $filtercfg2 `
                        + $ffmpegaudcmd + $ffmpegvcdctra + $ffmpegOutSrt

                        #Write-Host("**************************L5****************************")
                        $ffmpegCmdNom = $ffmpegCmd1 + $ffmpegCmdA + $ffmpegCmdV1 + $ffmpegCmdV2 `
                        + $filtercfg1 + $filtercfgX + $filtercfgY + $filtercfg2 `
                        + $ffmpegaudcmd + $ffmpegvcdcstd + $ffmpegOutNom
                        $ffmpegCmdEnd = $ffmpegCmd1 + $ffmpegCmdA + $ffmpegCmdV1 + $EndffmpegCmdV2 `
                        + $Endfiltercfg1 + $filtercfgX + $filtercfgY + $filtercfg2 `
                        + $ffmpegaudcmd + $ffmpegvcdctra + $ffmpegOutEnd

                        #Write-Host("**************************L8****************************")
                        #Execute the FFmpeg command
                        #write-host "ffmpeg command for image conversion:"
                        #write-host $ffmpegcmd
                        #$ffmpegCmdSrt | Out-File -FilePath "$($file.ImgVidPath)srtcmd"
                        if($ScaleWIM)
                        {
                            if ($SetTmpPath)
                            {
                                $TmpDirName = $SetTmpPath
                            }
                            else
                            {
                                $TmpDirName = $env:TEMP
                            }
                            $InputFilter = " -vf `"settb=expr=1/$VidRateTimescale,setpts=N/$frameRate/TB,fps=$frameRate`" "
                            $FFMPEGCmdSrtAppend = $InputFilter + $ffmpegaudcmd + $ffmpegvcdctra + $ffmpegOutSrtIM
                            $FFMPEGCmdNomAppend = $InputFilter + $ffmpegaudcmd + $ffmpegvcdcstd + $ffmpegOutNomIM
                            $FFMPEGCmdEndAppend = $InputFilter + $ffmpegaudcmd + $ffmpegvcdctra + $ffmpegOutEndIM
                            #write-host "About to call function..."
                            #write-host "ContPath: " + $file.ContPath
                            #write-host "IW: " + $image.Width
                            #write-host "IH: " + $image.Height
                            #write-host "StartZoom: " + $SetSrtZoom
                            #write-host "ZoomRate: " + $ZoomRate
                            #write-host "MaxSrtRot: " + $MaxSrtRot
                            #write-host "XRatio: " + $XRatio
                            #write-host "YRatio: " + $YRatio
                            #write-host "NFramesTrn: " + $NFramesTrn
                            #write-host "NFramesStd: " + $NFramesStd
                            #write-host "NFramesTrn: " + $XDim
                            #write-host "NFramesStd: " + $YDim
                            #write-host "FFMPEGCmdSrtAppend: " + $FFMPEGCmdSrtAppend
                            #write-host "FFMPEGCmdNomAppend: " + $FFMPEGCmdNomAppend
                            #write-host "FFMPEGCmdEndAppend: " + $FFMPEGCmdEndAppend
                            #write-host "TmpDirName: " + $TmpDirName
                            #Wait-Debugger
                            #New-VideoZoomedOutFromPic
                            New-VideoZoomedOutFromPic $file.ConvPath $file.ContPath $image.Width $image.Height $SetSrtZoom $ZoomRate $MaxSrtRot $XRatio $YRatio $NFramesTrn  $NFramesStd $XDim  $YDim $FFMPEGCmdSrtAppend $FFMPEGCmdNomAppend $FFMPEGCmdEndAppend $TmpDirName
                        }else{
                            (Invoke-Expression $ffmpegCmdSrt) *> $null
                            (Invoke-Expression $ffmpegCmdEnd) *> $null
                            (Invoke-Expression $ffmpegCmdNom) *> $null
                        }
                        #If each file has content, set the creation time.

                        [System.IO.File]::SetCreationTime( $srtpath, $CurrDateTime)
                        [System.IO.File]::SetCreationTime( $endpath, $CurrDateTime)
                        [System.IO.File]::SetCreationTime( $nompath, $CurrDateTime)
                        #Write-Host("**************************L9****************************")
                        #Now rewrite the image again with a smaller size, to save on space.
                        $IMCmd1 = "magick `"$( $file.ConvPath )`" -auto-orient -resize $SizeOut -quality $($quality.ToString() ) -background black "
                        $IMCmdOut = "`"$( $file.ContPath )`""
                        $IMCmd = $IMCmd1 + $PstCmd + $IMCmdOut
                        (Invoke-Expression $IMCmd) *> $null
                        [System.IO.File]::SetCreationTime( "$( $file.ContPath )", $CurrDateTime)
                    }
                }
                elseif($file.IsVid)
                {
                    $VPrams = ffprobe -v error -show_streams -select_streams v:0 -of ini $file.FullName
                    $VWidth = [Int]::0
                    $VWidth = [Int]::0
                    $Rotation = [Int]::0
                    $Duration = 0.0
                    $AudioSet = 0
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
                            if ( $Pram.StartsWith("rotation="))
                            {
                                $Rotation = [Int]::Parse($Pram.split('rotation=')[1])
                            }
                            if ( $Pram.StartsWith("duration="))
                            {
                                $Duration = [Decimal]::Parse($Pram.split('duration=')[1])
                            }
                            if ( $Pram.StartsWith("codec_type="))
                            {
                                $CodecType = ($Pram.split('codec_type='))[1]
                                if($CodecType -eq "audio")
                                {
                                    $AudioSet = 1
                                }
                            }
                        }
                        #If video is not oriented according to it's resolution, assume  a 90 deg turn.
                        if ($Rotation%180 -ne 0)
                        {
                            $VWidth = $PreHeight
                            $VHeight = $PreWidth
                        }
                        else
                        {
                            $VWidth = $PreWidth
                            $VHeight = $PreHeight
                        }
                        $whvidratio = $VWidth/$VHeight
                        #If width is greater, limit this dimension for resize.
                        if ($whvidratio -gt $whdispratio)
                        {
                            $contw = [int]$XDim
                            $conth = [int]($XDim/$whvidratio)
                        }
                        else
                        {
                            $conth = [int]$YDim
                            $contw = [int]($YDim*$whvidratio)
                        }
                        #If video packing, need to set the pad limits
                        if ($VidPack)
                        {
                            $Sides = $XDim - $contw;
                            $TopBot = $YDim - $conth;
                            $LBand = [math]::Floor($Sides/2)
                            $TBand = [math]::Floor($TopBot/2)
                            $RBand = $LBand
                            $BBand = $TBand

                            if ($Sides%2)
                            {
                                $RBand = $LBand + 1
                            }
                            if (($TopBot%2) -ge 1)
                            {
                                $BBand = $TBand + 1
                            }
                            $wint = $XDim -as [Int]
                            $hint = $YDim -as [Int]
                        }
                        else
                        {
                            $LBand = 0
                            $TBand = 0
                            $RBand = 0
                            $BBand = 0
                            if (($contw%2) -ge 1)
                            {
                                $contw = [math]::Ceiling($contw)
                            }
                            else
                            {
                                $contw = [math]::Floor($contw)
                            }
                            if (($conth%2) -ge 1)
                            {
                                $conth = [math]::Ceiling($conth)
                            }
                            else
                            {
                                $conth = [math]::Floor($conth)
                            }
                            $wint = $contw -as [Int]
                            $hint = $conth -as [Int]
                        }
                        $sizestr = $wint.ToString() + ":" + $hint.ToString()
                        #If bordering is required.
                        if ($LBand -or $RBand -or $TBand -or $BBand)
                        {
                            $PadOpt = ",pad=" + $sizestr + "`:$LBand`:$TBand,setsar=1"
                        }
                        else
                        {
                            $PadOpt = ""
                        }
                        $nomdur = $Duration - ($SelFadeTime*2)
                        $endsrt = $Duration - $SelFadeTime
                        $NFramesNom = [math]::floor($nomdur*$framerate)
                        if ($SelFadeTime -lt ($nomdur/2)){$AFd = $SelFadeTime }
                        else{$AFd = ($nomdur/2)}
                        $AOtOf = $nomdur - $AFd
                        $ffmpegCmdA = "-f lavfi -i anullsrc  -loop 1" #Only when audio stream needs to be created.
                        $ffmpeginputsrt = "ffmpeg -y -t $SelFadeTime -i `"$( $file.FullName )`" "
                        $ffmpeginputnom = "ffmpeg -y -ss $SelFadeTime -t $nomdur -i `"$( $file.FullName )`" "
                        $ffmpeginputend = "ffmpeg -y -ss $endsrt -t $SelFadeTime -i `"$( $file.FullName )`" "
                        $ffmpegvidfiltsub = "scale=$wint`:$hint`:force_original_aspect_ratio=decrease$PadOpt"
                        $ffmpegvidfilt = "fps=fps=$framerate[vint]`;[vint]$ffmpegvidfiltsub"
                        $ffmpegvidcmd1 = "-vf "+ $ffmpegvidfiltsub
                        $ffmpegcmdsrt = $ffmpeginputsrt + $ffmpegvidcmd1 + " " + $ffmpegaudcmd + $ffmpegvcdctra + " -movflags faststart `"$srtpath`""
                        $ffmpegcmdend = $ffmpeginputend + $ffmpegvidcmd1 + " " + $ffmpegaudcmd + $ffmpegvcdctra + " -movflags faststart `"$endpath`""
                        if($AudioSet)
                        {$ffmpegcmdnom = $ffmpeginputnom + "-filter_complex `"[0:v]$ffmpegvidfilt`;[0:a]afade=t=in:st=0:d=$AFd,afade=t=out:st=$AOtOf`:d=$AFd`" " + $ffmpegaudcmd + $ffmpegvcdcstd + " `"$nompath`""}
                        else
                        {$ffmpegcmdnom = $ffmpeginputnom + " $ffmpegCmdA " + "-filter_complex `"[0:v]$ffmpegvidfilt[vout]`" " + $ffmpegaudcmd + $ffmpegvcdcstd + " -map `"[vout]`" -map 1:a -frames:v $NFramesNom `"$nompath`""}

                        #Uncomment to generate files with commands for debugging
                        #$ffmpegcmdnom | Out-File -FilePath "$($file.ContPath)nomcmd"
                        #$ffmpegcmdsrt | Out-File -FilePath "$($file.ContPath)srtcmd"
                        #$ffmpegcmdend | Out-File -FilePath "$($file.ContPath)endcmd"
                        (Invoke-Expression $ffmpegcmdsrt) *> $null
                        (Invoke-Expression $ffmpegcmdend) *> $null
                        (Invoke-Expression $ffmpegcmdnom) *> $null
                        #Get file size of nominal file, if this is non-zero, update the modificatoin time so the tool knows it's up-to-date, else remove the files.
                        if(Get-ChildItem -Path $( $file.ContPath) | Select-Object Length)
                        {
                            [System.IO.File]::SetCreationTime($srtpath, $CurrDateTime)
                            [System.IO.File]::SetCreationTime($endpath, $CurrDateTime)
                            [System.IO.File]::SetCreationTime($nompath, $CurrDateTime)
                        }
                        $file.ExpDefComplete = 1
                        $_.ExpDefComplete = 1

                    }
                }
            }
            catch
            {
                if ($file.ImgVidPath.length)
                {
                    write-host "Image conversion failed"
                    write-host "ContPath: " + $file.ContPath
                    write-host "IW: " + $image.Width
                    write-host "IH: " + $image.Height
                    write-host "StartZoom: " + $SetSrtZoom
                    write-host "ZoomRate: " + $ZoomRate
                    write-host "MaxSrtRot: " + $MaxSrtRot
                    write-host "XRatio: " + $XRatio
                    write-host "YRatio: " + $YRatio
                    write-host "NFramesTrn: " + $NFramesTrn
                    write-host "NFramesStd: " + $NFramesStd
                    write-host "NFramesTrn: " + $XDim
                    write-host "NFramesStd: " + $YDim
                    write-host "FFMPEGCmdSrtAppend: " + $FFMPEGCmdSrtAppend
                    write-host "FFMPEGCmdNomAppend: " + $FFMPEGCmdNomAppend
                    write-host "FFMPEGCmdEndAppend: " + $FFMPEGCmdEndAppend
                    write-host "TmpDirName: " + $TmpDirName
                }
                else
                {
                    write-host "Video conversion failed"
                    write-host "ContPath: " + $file.ContPath
                }
            }
            if ($ShowProg)
            {
                $LoopProg += $file.Length
                $CurrInnerProgPercInt[0] = ($LoopProg*100)/$AllFilesizeTtl
                if ($CurrInnerProgPercInt[0] -gt $PrevInnerProgPercInt[0])
                {
                    $InnerLoopProg.PercentComplete = $CurrInnerProgPercInt[0]
                    $PrevInnerProgPercInt[0] = $CurrInnerProgPercInt[0]
                    $InnerLoopProg.Status = "Creating content files: " + $InnerLoopProg.PercentComplete.ToString() + "% Complete"
                    Write-Progress @InnerLoopProg
                }
            }
        #}
        } -ThrottleLimit 4
    }
}