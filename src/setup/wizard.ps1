# First-run setup wizard (WinForms) for make_video_slideshow.
# Implements: LLR-029, SR-026
# Collects the basics and prints them as key=value lines to stdout on OK.
# Exit 0 = submitted (values on stdout); exit 1 = cancelled/closed.
# Requires Windows PowerShell with WinForms; launch with -STA.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

function New-Label($text, $y) {
    $l = New-Object System.Windows.Forms.Label
    $l.Text = $text; $l.AutoSize = $true
    $l.Location = New-Object System.Drawing.Point(12, ($y + 3))
    return $l
}
function New-Text($value, $y, $width = 250) {
    $t = New-Object System.Windows.Forms.TextBox
    $t.Text = "$value"
    $t.Location = New-Object System.Drawing.Point(170, $y)
    $t.Size = New-Object System.Drawing.Size($width, 22)
    return $t
}

$form = New-Object System.Windows.Forms.Form
$form.Text = "make_video_slideshow - first run setup"
$form.Size = New-Object System.Drawing.Size(520, 430)
$form.StartPosition = "CenterScreen"
$form.FormBorderStyle = "FixedDialog"
$form.MaximizeBox = $false; $form.MinimizeBox = $false

$y = 15
$form.Controls.Add((New-Label "Photos/videos folder (source):" $y))
$srcBox = New-Text "" $y 230; $form.Controls.Add($srcBox)
$srcBtn = New-Object System.Windows.Forms.Button
$srcBtn.Text = "Browse..."; $srcBtn.Location = New-Object System.Drawing.Point(410, ($y - 1))
$srcBtn.Size = New-Object System.Drawing.Size(80, 24)
$srcBtn.Add_Click({
    $d = New-Object System.Windows.Forms.FolderBrowserDialog
    if ($d.ShowDialog() -eq 'OK') { $srcBox.Text = $d.SelectedPath }
})
$form.Controls.Add($srcBtn)

$y += 35
$form.Controls.Add((New-Label "Output folder (videos):" $y))
$outBox = New-Text "" $y 230; $form.Controls.Add($outBox)
$outBtn = New-Object System.Windows.Forms.Button
$outBtn.Text = "Browse..."; $outBtn.Location = New-Object System.Drawing.Point(410, ($y - 1))
$outBtn.Size = New-Object System.Drawing.Size(80, 24)
$outBtn.Add_Click({
    $d = New-Object System.Windows.Forms.FolderBrowserDialog
    if ($d.ShowDialog() -eq 'OK') { $outBox.Text = $d.SelectedPath }
})
$form.Controls.Add($outBtn)

$y += 35
$form.Controls.Add((New-Label "Temp folder (optional):" $y))
$tmpBox = New-Text "" $y 230; $form.Controls.Add($tmpBox)

$y += 35
$form.Controls.Add((New-Label "Frame width (px):" $y))
$wBox = New-Text "1440" $y 100; $form.Controls.Add($wBox)
$y += 30
$form.Controls.Add((New-Label "Frame height (px):" $y))
$hBox = New-Text "900" $y 100; $form.Controls.Add($hBox)
$y += 30
$form.Controls.Add((New-Label "Frames per second:" $y))
$fpsBox = New-Text "30" $y 100; $form.Controls.Add($fpsBox)
$y += 30
$form.Controls.Add((New-Label "Quality (CRF, 26-30 typical):" $y))
$crfBox = New-Text "28" $y 100; $form.Controls.Add($crfBox)
$y += 30
$form.Controls.Add((New-Label "Seconds per image:" $y))
$dispBox = New-Text "6" $y 100; $form.Controls.Add($dispBox)
$y += 30
$form.Controls.Add((New-Label "Number of frames (displays):" $y))
$framesBox = New-Text "1" $y 100; $form.Controls.Add($framesBox)

$y += 45
$ok = New-Object System.Windows.Forms.Button
$ok.Text = "Create config"; $ok.Location = New-Object System.Drawing.Point(250, $y)
$ok.Size = New-Object System.Drawing.Size(110, 28)
$ok.DialogResult = [System.Windows.Forms.DialogResult]::OK
$cancel = New-Object System.Windows.Forms.Button
$cancel.Text = "Cancel"; $cancel.Location = New-Object System.Drawing.Point(370, $y)
$cancel.Size = New-Object System.Drawing.Size(110, 28)
$cancel.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
$form.Controls.Add($ok); $form.Controls.Add($cancel)
$form.AcceptButton = $ok; $form.CancelButton = $cancel

# Basic validation: require source + output before accepting.
$ok.Add_Click({
    if ([string]::IsNullOrWhiteSpace($srcBox.Text) -or [string]::IsNullOrWhiteSpace($outBox.Text)) {
        [System.Windows.Forms.MessageBox]::Show("Please choose a source folder and an output folder.", "Missing folders") | Out-Null
        $form.DialogResult = [System.Windows.Forms.DialogResult]::None
    }
})

$result = $form.ShowDialog()
if ($result -ne [System.Windows.Forms.DialogResult]::OK) { exit 1 }

# Emit collected values as key=value lines on stdout for the Rust caller to parse.
Write-Output "source=$($srcBox.Text)"
Write-Output "output_dir=$($outBox.Text)"
Write-Output "temp_dir=$($tmpBox.Text)"
Write-Output "width=$($wBox.Text)"
Write-Output "height=$($hBox.Text)"
Write-Output "fps=$($fpsBox.Text)"
Write-Output "quality_crf=$($crfBox.Text)"
Write-Output "pic_display_time_secs=$($dispBox.Text)"
Write-Output "frames=$($framesBox.Text)"
exit 0
