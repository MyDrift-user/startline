# Builds the release files into dist/, from what `cargo build --release` left in target\. Needs VERSION, Inno Setup and WiX.
$ErrorActionPreference = 'Stop'
$base = "startline-v$env:VERSION-windows-x64"
New-Item -ItemType Directory -Force dist | Out-Null
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" /Q "/DAppVersion=$env:VERSION" "/DOutputName=$base-setup" installer\windows\setup.iss
if ($LASTEXITCODE) { throw "ISCC failed" }
# The wizard shows the licence as RTF, which is plain text with its special characters escaped.
$lines = (Get-Content LICENSE) -replace '\\', '\\' -replace '\{', '\{' -replace '\}', '\}'
Set-Content target\license.rtf ('{\rtf1\ansi\deff0{\fonttbl{\f0 Consolas;}}\f0\fs15 ' + ($lines -join "\par`n") + '}') -Encoding ascii
# One MSI whose wizard is in the language Windows is shown in. It's built in each language with one ProductCode; the
# German and Italian ones become language transforms, named by their language ID, inside the English one, which lists all
# three: Windows then applies the one for its own language. Done through Windows Installer itself, as Microsoft's
# WiSubStg.vbs and WiLangId.vbs do, so nothing beyond WiX is needed.
$msiDir = 'target\msi'
# empty, as Windows Installer won't write a transform over one a build before left here (the update test packages twice)
Remove-Item -Recurse -Force $msiDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $msiDir | Out-Null
$productCode = "{$([guid]::NewGuid().ToString().ToUpper())}"
$languages = [ordered]@{ 'en-US' = 1033; 'de-DE' = 1031; 'it-IT' = 1040 }
foreach ($culture in $languages.Keys) {
    wix build -arch x64 -ext WixToolset.UI.wixext -culture $culture -loc "installer\windows\$culture.wxl" -d "Version=$env:VERSION" -d "ProductCode=$productCode" -o "$msiDir\$culture.msi" installer\windows\app.wxs
    if ($LASTEXITCODE) { throw "wix failed for $culture" }
}
$installer = New-Object -ComObject WindowsInstaller.Installer
# Late-bound calls on Windows Installer's objects. PowerShell wraps what it passes along, which COM can't take: unwrapped.
function Bare([object[]]$arguments) { ,[object[]]@($arguments | ForEach-Object { if ($_ -is [Management.Automation.PSObject]) { $_.psobject.BaseObject } else { $_ } }) }
function Call($target, [string]$name, [object[]]$arguments) { $target.GetType().InvokeMember($name, 'InvokeMethod', $null, $target, (Bare $arguments)) }
function Get($target, [string]$name, [object[]]$arguments) { $target.GetType().InvokeMember($name, 'GetProperty', $null, $target, (Bare $arguments)) }
function Put($target, [string]$name, [object[]]$arguments) { $target.GetType().InvokeMember($name, 'SetProperty', $null, $target, (Bare $arguments)) | Out-Null }
$english = (Resolve-Path "$msiDir\en-US.msi").Path
foreach ($culture in 'de-DE', 'it-IT') {
    $id = $languages[$culture]
    $mst = Join-Path (Resolve-Path $msiDir).Path "$id.mst"
    # what turns the English MSI into this language's (read-only: 0)
    $reference = Call $installer 'OpenDatabase' @($english, 0)
    $localized = Call $installer 'OpenDatabase' @((Resolve-Path "$msiDir\$culture.msi").Path, 0)
    if (-not (Call $localized 'GenerateTransform' @($reference, $mst))) { throw "No differences for $culture" }
    Call $localized 'CreateTransformSummaryInfo' @($reference, $mst, 0, 0) | Out-Null
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($localized)
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($reference)
    # stored in the English MSI under its language ID (transacted: 1)
    $db = Call $installer 'OpenDatabase' @($english, 1)
    $view = Call $db 'OpenView' @('SELECT `Name`, `Data` FROM `_Storages`')
    $record = Call $installer 'CreateRecord' @(2)
    Put $record 'StringData' @(1, "$id")
    Call $record 'SetStream' @(2, $mst) | Out-Null
    Call $view 'Execute' @() | Out-Null
    Call $view 'Modify' @(3, $record) | Out-Null  # assign
    Call $view 'Close' @() | Out-Null
    Call $db 'Commit' @() | Out-Null
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($view)
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($db)
}
# the languages the package offers, after its platform: "x64;1033,1031,1040"
$db = Call $installer 'OpenDatabase' @($english, 1)
$summary = Get $db 'SummaryInformation' @(1)
$template = Get $summary 'Property' @(7)
Put $summary 'Property' @(7, ($template.Split(';')[0] + ';' + ($languages.Values -join ',')))
Call $summary 'Persist' @() | Out-Null
Call $db 'Commit' @() | Out-Null
[void][Runtime.InteropServices.Marshal]::ReleaseComObject($summary)
[void][Runtime.InteropServices.Marshal]::ReleaseComObject($db)
"MSI languages: $(Get (Get (Call $installer 'OpenDatabase' @($english, 0)) 'SummaryInformation' @(0)) 'Property' @(7))"
Copy-Item $english "dist\$base.msi"
Copy-Item target\release\startline.exe "dist\$base-portable.exe"
Compress-Archive -Path target\release\startline.exe, LICENSE -DestinationPath "dist\$base.zip"
Compress-Archive -Path PolicyDefinitions\* -DestinationPath "dist\startline-v$env:VERSION-policy-definitions.zip"
Copy-Item LICENSE "dist\startline-v$env:VERSION-LICENSE.txt"
