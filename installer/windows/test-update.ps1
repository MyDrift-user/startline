# Installs the old\ build every way it can be installed and updates it to the www\ build from a local server, with the
# real installers and the real SYSTEM task. See build-two-versions.ps1. Needs VERSION, and an admin prompt.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\test-lib.ps1"

$new = '99.0.0'
$oldBase = (Resolve-Path 'old').Path + "\startline-v$env:VERSION-windows-x64"
$www = (Resolve-Path 'www').Path
$newBase = "$www\startline-v$new-windows-x64"
$task = 'Startline Updater'
$systemLog = "$env:windir\System32\config\systemprofile\AppData\Local\Startline"
$ownLog = "$env:LOCALAPPDATA\Startline"
$policy = 'HKLM:\SOFTWARE\Policies\Startline'
$switch = "$env:ProgramData\Startline\auto-update.txt"
$defaultFolder = "$env:ProgramFiles\Startline"

Set-Content 'C:\startline-update-test.txt' 'http://127.0.0.1:18766'
$server = Start-Process python -ArgumentList '-m', 'http.server', '18766', '--bind', '127.0.0.1', '--directory', "`"$www`"" -PassThru -WindowStyle Hidden
Start-Sleep 3

function Say($text) { Write-Host "=== $text" }

function Assert-Version($exe, $version) {
  $got = Version $exe
  if ($got -ne "Startline $version") { throw "$exe is '$got', not $version" }
}

# Where the installer of the update being tested writes its log.
$script:installerLog = $null

function Show-Logs {
  foreach ($log in "$systemLog\update.log", "$ownLog\update.log", $script:installerLog) {
    if ($log -and (Test-Path $log)) {
      Write-Host "--- $log"
      # an installer's log is long, so show what led to its first failure
      $failed = Select-String -Path $log -Pattern 'Return value 3|Error \d{4}\.|^.*error occurred' | Select-Object -First 1
      if ($failed) { Get-Content $log | Select-Object -Skip ([Math]::Max(0, $failed.LineNumber - 30)) -First 45 | Write-Host }
      Get-Content $log -Tail 25 | Write-Host
    }
  }
}

# The installer, or the helper copy that runs it.
function Installer-Running { @(Get-CimInstance Win32_Process | Where-Object { ($_.CommandLine -match "startline-v$new" -or $_.Name -eq 'startline-update-helper.exe') -and $_.ProcessId -ne $PID -and $_.Name -notmatch 'python|pwsh|powershell' }).Count -gt 0 }

# Waits for the installer the update started, then checks that it says it worked.
function Wait-Updated($exe) {
  foreach ($i in 1..72) {
    Start-Sleep 5
    if ((Installer-Running)) { continue }
    try { if ((Version $exe) -eq "Startline $new") { break } } catch {}
    if ($i -eq 72) {
      Show-Logs
      # what's there instead, and what the installer decided about it
      try { Write-Host "--- it says: '$(Version $exe)'" } catch { Write-Host "--- it doesn't run: $_" }
      Get-Item $exe -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "--- $($_.FullName) $($_.Length) bytes, written $($_.LastWriteTime), version $($_.VersionInfo.FileVersion)" }
      Get-ChildItem (Split-Path $exe) -Recurse -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "    $($_.FullName)" }
      if ($script:installerLog -and (Test-Path $script:installerLog)) {
        Select-String -Path $script:installerLog -Pattern 'startline\.exe|APPLICATIONFOLDER|RemoveExistingProducts|Won.t|Disallow' | Select-Object -First 60 | ForEach-Object { Write-Host $_.Line }
      }
      throw "$exe didn't update"
    }
  }
  Assert-Version $exe $new
  if ($script:installerLog -notlike '*.log' -or -not (Test-Path $script:installerLog)) { throw "The installer left no log at $script:installerLog" }
  # an in-use file only gets replaced at the next restart
  if ((Get-Content -Raw $script:installerLog) -match 'requires a system restart|ReplacedInUseFiles|Need to restart Windows\? Yes') { Show-Logs; throw "The update needs a restart" }
}

function Install-Msi($msi, [string[]]$properties) {
  $p = Start-Process msiexec -ArgumentList (@('/i', "`"$msi`"", '/qn', '/norestart') + $properties) -Wait -PassThru
  if ($p.ExitCode) { throw "$msi didn't install: $($p.ExitCode)" }
}

function Remove-Msi($msi, [string[]]$properties) {
  $p = Start-Process msiexec -ArgumentList (@('/x', "`"$msi`"", '/qn', '/norestart') + $properties) -Wait -PassThru
  if ($p.ExitCode) { throw "$msi didn't uninstall: $($p.ExitCode)" }
}

function Install-Setup([string[]]$switches, $base = $oldBase) {
  $p = Start-Process "$base-setup.exe" -ArgumentList (@('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') + $switches) -Wait -PassThru
  if ($p.ExitCode) { throw "$base-setup.exe didn't install: $($p.ExitCode)" }
}

function Remove-Setup($folder) {
  $p = Start-Process "$folder\unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
  if ($p.ExitCode) { throw "setup.exe didn't uninstall: $($p.ExitCode)" }
  Start-Sleep 5
}

function Listed($root) { @(Get-ItemProperty "$root\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue | Where-Object DisplayName -eq 'Startline') }

# The task installs the update for a PC-wide install, which stays where it is, locked, listed once and with its task.
function Test-Machine($folder) {
  $exe = "$folder\startline.exe"
  Assert-Version $exe $env:VERSION
  $script:installerLog = "$folder\updates\installer.log"
  schtasks /Run /TN $task | Out-Null
  if ($LASTEXITCODE) { throw "The task didn't start" }
  Wait-Updated $exe
  Assert-Locked $folder
  if ((Listed 'HKLM:').Count -ne 1) { throw "Windows lists Startline $((Listed 'HKLM:').Count) times" }
  $xml = schtasks /Query /TN $task /XML | Out-String
  if ($LASTEXITCODE -or -not $xml.Contains("$folder\startline.exe")) { throw "After updating, the task doesn't run $exe" }
  if ($folder -ne $defaultFolder -and (Test-Path $defaultFolder)) { throw "The update also installed to the default folder" }
  # with nothing new to do, it removes its download
  schtasks /Run /TN $task | Out-Null
  Start-Sleep 15
  if (Get-ChildItem "$folder\updates" -File -ErrorAction SilentlyContinue | Where-Object Name -like '*99.0.0*') { throw "The task kept the installer it used" }
}

function Assert-Gone($folder) {
  if (Test-Path $folder) { throw "Uninstalling left $folder behind: $((Get-ChildItem -Recurse $folder).FullName -join ', ')" }
  schtasks /Query /TN $task 2>$null | Out-Null
  if ($LASTEXITCODE -eq 0) { throw "Uninstalling left the task" }
  $global:LASTEXITCODE = 0
}

# An update that must not install, as the task's program would run it: it says why in the log, or exits with a failure.
function Assert-Stays($exe, $expectedCode, $logText) {
  Remove-Item "$ownLog\update.log" -ErrorAction SilentlyContinue
  $r = Run $exe '--update-service'
  if ($r.Code -ne $expectedCode) { Show-Logs; throw "The update service exited with $($r.Code), not $expectedCode" }
  if (-not (Select-String -Quiet -SimpleMatch $logText "$ownLog\update.log")) { Show-Logs; throw "The log doesn't say '$logText'" }
  if (Installer-Running) { throw "An installer started" }
  Assert-Version $exe $env:VERSION
}

function Clear-Policy {
  Remove-Item $policy -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item $switch -Force -ErrorAction SilentlyContinue
}

try {
  Say 'The templates are valid XML'
  foreach ($file in 'PolicyDefinitions\Startline.admx', 'PolicyDefinitions\en-US\Startline.adml') { [void][xml](Get-Content -Raw $file) }

  Say 'PC-wide MSI, default folder'
  Install-Msi "$oldBase.msi" 'ALLUSERS=1'
  $exe = "$defaultFolder\startline.exe"
  Test-Machine $defaultFolder
  $target = (New-Object -ComObject WScript.Shell).CreateShortcut("$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Startline.lnk").TargetPath
  if ($target -ne $exe) { throw "After updating, the shortcut points to '$target'" }
  Remove-Msi "$newBase.msi" 'ALLUSERS=1'
  Assert-Gone $defaultFolder

  Say 'PC-wide MSI, chosen folder'
  $chosen = 'C:\Apps\Startline MSI Test'
  Install-Msi "$oldBase.msi" 'ALLUSERS=1', "APPLICATIONFOLDER=`"$chosen`""
  Test-Machine $chosen
  Remove-Msi "$newBase.msi" 'ALLUSERS=1'
  Assert-Gone $chosen

  Say 'PC-wide setup.exe, default folder'
  Install-Setup '/ALLUSERS'
  Test-Machine $defaultFolder
  Remove-Setup $defaultFolder
  Assert-Gone $defaultFolder

  Say 'PC-wide setup.exe, chosen folder'
  $chosen = 'C:\Apps\Startline Setup Test'
  Install-Setup '/ALLUSERS', "/DIR=`"$chosen`""
  Test-Machine $chosen
  Remove-Setup $chosen
  Assert-Gone $chosen

  Say 'What stops the task from updating'
  Install-Msi "$oldBase.msi" 'ALLUSERS=1'
  $off = 'automatic updates are off'

  New-Item $policy -Force | Out-Null
  Set-ItemProperty $policy CheckForUpdates 0 -Type DWord
  schtasks /Run /TN $task | Out-Null
  Start-Sleep 25
  if (Installer-Running) { throw "CheckForUpdates: the task started an installer" }
  Assert-Version $exe $env:VERSION
  if (-not (Select-String -Quiet -SimpleMatch $off "$systemLog\update.log")) { Show-Logs; throw "CheckForUpdates: the task didn't say it's off" }
  $r = Run $exe '--update-now'
  if ($r.Text -notmatch 'managed by the administrator') { throw "CheckForUpdates: --update-now said '$($r.Text)'" }
  Clear-Policy

  New-Item $policy -Force | Out-Null
  Set-ItemProperty $policy AutomaticUpdates 0 -Type DWord
  Assert-Stays $exe 0 $off
  New-Item (Split-Path $switch) -ItemType Directory -Force | Out-Null
  Set-Content $switch '1'
  Assert-Stays $exe 0 $off
  Clear-Policy

  New-Item (Split-Path $switch) -ItemType Directory -Force | Out-Null
  Set-Content $switch '0'
  Assert-Stays $exe 0 $off
  Clear-Policy

  Say 'What must not be installed'
  $list = "$www\latest.json"
  $signature = "$www\latest.json.sig"
  $goodList = [IO.File]::ReadAllBytes($list)
  $goodSignature = [IO.File]::ReadAllBytes($signature)
  $restore = { [IO.File]::WriteAllBytes($list, $goodList); [IO.File]::WriteAllBytes($signature, $goodSignature) }
  $sign = { $null = Run "$oldBase-portable.exe" "--test-sign `"$list`" `"$signature`"" }

  [IO.File]::WriteAllBytes($signature, (New-Object byte[] 64))
  Assert-Stays $exe 1 "signature doesn't match"
  & $restore

  [IO.File]::AppendAllText($list, "file 00 evil.exe`n")
  Assert-Stays $exe 1 "signature doesn't match"
  & $restore

  # signed, but the file isn't what the list says
  $text = (Get-Content -Raw $list) -replace '(?m)^file [0-9a-f]+ (startline-v.*\.msi)\s*$', ('file ' + ('0' * 64) + ' $1')
  [IO.File]::WriteAllText($list, $text)
  & $sign
  Assert-Stays $exe 1 "didn't match its signed checksum"
  & $restore

  # signed, but older
  [IO.File]::WriteAllText($list, "version 0.0.0`n")
  & $sign
  Assert-Stays $exe 0 'install type Machine'
  & $restore

  Say 'Someone without admin rights on a PC-wide install'
  $name = 'startlinetest'
  $password = "Te5t-$(New-Guid)"
  $secure = ConvertTo-SecureString $password -AsPlainText -Force
  New-LocalUser $name -Password $secure | Out-Null
  $credential = [pscredential]::new($name, $secure)
  # its own folder for the log and the update list, whichever environment it starts with
  $home2 = 'C:\startline-test-home'
  New-Item $home2 -ItemType Directory -Force | Out-Null
  icacls $home2 /grant '*S-1-1-0:(OI)(CI)F' | Out-Null
  $saved = $env:LOCALAPPDATA
  $env:LOCALAPPDATA = $home2
  $r = Run $exe '--update-now' $credential
  $env:LOCALAPPDATA = $saved
  Remove-LocalUser $name
  if ($r.Code -or $r.Text -notmatch 'InstallsByTask') { throw "A normal user's check said '$($r.Text)', exit $($r.Code)" }
  Assert-Version $exe $env:VERSION

  Say 'A policy that turns automatic updates on beats the switch'
  New-Item $policy -Force | Out-Null
  Set-ItemProperty $policy AutomaticUpdates 1 -Type DWord
  New-Item (Split-Path $switch) -ItemType Directory -Force | Out-Null
  Set-Content $switch '0'
  Test-Machine $defaultFolder
  Clear-Policy
  Remove-Msi "$newBase.msi" 'ALLUSERS=1'
  Assert-Gone $defaultFolder

  $own = "$env:LOCALAPPDATA\Programs\Startline"
  foreach ($test in @(
      @{ Name = 'One account, MSI, default folder'; Folder = $own; Msi = $true; Extra = @() },
      @{ Name = 'One account, MSI, chosen folder'; Folder = "$env:LOCALAPPDATA\Startline MSI Test"; Msi = $true; Extra = @("APPLICATIONFOLDER=`"$env:LOCALAPPDATA\Startline MSI Test`"") },
      @{ Name = 'One account, setup.exe, default folder'; Folder = $own; Msi = $false; Extra = @() },
      @{ Name = 'One account, setup.exe, chosen folder'; Folder = "$env:LOCALAPPDATA\Startline Setup Test"; Msi = $false; Extra = @("/DIR=`"$env:LOCALAPPDATA\Startline Setup Test`"") })) {
    Say $test.Name
    $perUser = 'ALLUSERS=2', 'MSIINSTALLPERUSER=1'
    if ($test.Msi) { Install-Msi "$oldBase.msi" ($perUser + $test.Extra) } else { Install-Setup (@('/CURRENTUSER') + $test.Extra) }
    $exe = "$($test.Folder)\startline.exe"
    Assert-Version $exe $env:VERSION
    $script:installerLog = "$ownLog\updates\installer.log"
    $r = Run $exe '--update-now'
    if ($r.Code -or $r.Text -notmatch 'Installing') { Show-Logs; throw "--update-now said '$($r.Text)', exit $($r.Code)" }
    Wait-Updated $exe
    if ($test.Folder -ne $own -and (Test-Path "$own\startline.exe")) { throw "The update also installed to the default folder" }
    if (Test-Path "$($test.Folder)\machine-install.txt") { throw "It became a PC-wide install" }
    schtasks /Query /TN $task 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { throw "An install for one account added the task" }
    $global:LASTEXITCODE = 0
    if ($test.Msi) { Remove-Msi "$newBase.msi" $perUser } else { Remove-Setup $test.Folder }
    if (Test-Path $exe) { throw "$exe is still there after uninstalling" }
  }

  # The installer's Microsoft 365 app registration: given to the old version, kept by an upgrade to the new one that
  # doesn't give it again, used by the program, and removed when the new one is uninstalled.
  $m365Msi = "M365CLIENTID=$TestClientId", "M365TENANTID=$TestTenant"
  $m365Setup = "/M365CLIENTID=$TestClientId", "/M365TENANTID=$TestTenant"
  $own = "$env:LOCALAPPDATA\Programs\Startline"
  foreach ($test in @(
      @{ Name = 'MSI for this PC'; Root = 'HKLM:'; Folder = $defaultFolder; Msi = $true; Scope = @('ALLUSERS=1') },
      @{ Name = 'MSI for one account'; Root = 'HKCU:'; Folder = $own; Msi = $true; Scope = @('ALLUSERS=2', 'MSIINSTALLPERUSER=1') },
      @{ Name = 'setup.exe for this PC'; Root = 'HKLM:'; Folder = $defaultFolder; Msi = $false; Scope = @('/ALLUSERS') },
      @{ Name = 'setup.exe for one account'; Root = 'HKCU:'; Folder = $own; Msi = $false; Scope = @('/CURRENTUSER') })) {
    Say "Microsoft 365 app registration, $($test.Name)"
    Clear-M365Values
    if ($test.Msi) { Install-Msi "$oldBase.msi" ($test.Scope + $m365Msi) } else { Install-Setup ($test.Scope + $m365Setup) }
    Assert-M365Values $test.Root $TestClientId $TestTenant "$($test.Name), installed with them"
    if ($test.Msi) { Install-Msi "$newBase.msi" $test.Scope } else { Install-Setup $test.Scope $newBase }
    $exe = "$($test.Folder)\startline.exe"
    Assert-Version $exe $new
    Assert-M365Values $test.Root $TestClientId $TestTenant "$($test.Name), upgraded without them"
    Assert-M365FromInstaller $exe
    # setup.exe run again with another one replaces it (an MSI of the same version only repairs, so it's left out)
    if (-not $test.Msi) {
      $other = '11111111-2222-4333-8444-555555555555'
      Install-Setup ($test.Scope + "/M365CLIENTID=$other") $newBase
      Assert-M365Values $test.Root $other $TestTenant "$($test.Name), installed again with another one"
    }
    if ($test.Msi) { Remove-Msi "$newBase.msi" $test.Scope } else { Remove-Setup $test.Folder }
    Assert-M365Values $test.Root $null $null "$($test.Name), uninstalled"
  }

  Say 'Portable'
  $portable = "$env:TEMP\portable\startline.exe"
  New-Item (Split-Path $portable) -ItemType Directory -Force | Out-Null
  Copy-Item "$oldBase-portable.exe" $portable
  $r = Run $portable '--update-now'
  if ($r.Code -or $r.Text -notmatch 'Installing') { Show-Logs; throw "--update-now said '$($r.Text)', exit $($r.Code)" }
  Assert-Version $portable $new

  Say 'Everything passed'
} catch {
  Show-Logs
  throw
} finally {
  if (-not $server.HasExited) { $server.Kill() }
  Remove-Item 'C:\startline-update-test.txt' -ErrorAction SilentlyContinue
}
$global:LASTEXITCODE = 0
