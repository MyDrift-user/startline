# What the installer tests share.

# Runs the program and returns its exit code and what it printed. It's a windowed program, so it's waited for explicitly.
function Run($exe, $arguments, $credential) {
  $out = New-TemporaryFile
  $options = @{ FilePath = $exe; ArgumentList = $arguments; Wait = $true; PassThru = $true; RedirectStandardOutput = $out.FullName }
  if ($credential) { $options.Credential = $credential; $options.LoadUserProfile = $true; $options.WorkingDirectory = "$env:windir\Temp" }
  $p = Start-Process @options
  [pscustomobject]@{ Code = $p.ExitCode; Text = (Get-Content -Raw $out.FullName) }
}

function Version($exe) { "$((Run $exe '--version').Text)".Trim() }

# A made-up Microsoft 365 app registration and tenant, for checking that the installers keep what they're given.
$TestClientId = '0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0'
$TestTenant = 'contoso.onmicrosoft.com'

# What the installer left in Startline's own key under HKLM: or HKCU:, compared with what it should be ($null: nothing).
function Assert-M365Values($root, $client, $tenant, $what) {
  $key = Get-ItemProperty "$root\SOFTWARE\Startline" -ErrorAction SilentlyContinue
  if ("$($key.M365ClientId)" -ne "$client" -or "$($key.M365TenantId)" -ne "$tenant") {
    throw "${what}: $root\SOFTWARE\Startline holds '$($key.M365ClientId)' and '$($key.M365TenantId)', not '$client' and '$tenant'"
  }
}

# The installed program has to find them, and say they came from the installer.
function Assert-M365FromInstaller($exe) {
  $said = (Run $exe '--m365-source').Text
  if ($said -notmatch "client Installer $TestClientId" -or $said -notmatch "tenant Installer $TestTenant") { throw "$exe uses: $said" }
}

# Nothing left over from an earlier test.
function Clear-M365Values {
  foreach ($root in 'HKLM:', 'HKCU:') {
    Remove-ItemProperty "$root\SOFTWARE\Startline" -Name M365ClientId, M365TenantId -ErrorAction SilentlyContinue
  }
}

# Only admins and SYSTEM may change a folder whose program the update task runs as SYSTEM, and it still has to run.
function Assert-Locked($folder) {
  if (-not ((Version "$folder\startline.exe") -like 'Startline *')) { throw "The program in $folder doesn't run" }
  foreach ($path in @($folder) + @(Get-ChildItem $folder -Recurse | ForEach-Object FullName)) {
    $rules = (Get-Acl $path).Access
    $users = $rules | Where-Object { $_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -eq 'S-1-5-32-545' }
    if (-not ($users.FileSystemRights.value__ -band [int][Security.AccessControl.FileSystemRights]::ReadAndExecute)) { throw "Users can't read $path" }
    foreach ($rule in $rules) {
      $who = $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
      $writes = $rule.FileSystemRights.value__ -band ([int][Security.AccessControl.FileSystemRights]::Write -bor [int][Security.AccessControl.FileSystemRights]::Delete -bor [int][Security.AccessControl.FileSystemRights]::ChangePermissions -bor [int][Security.AccessControl.FileSystemRights]::TakeOwnership)
      if ($rule.AccessControlType -eq 'Allow' -and $writes -and $who -notin 'S-1-5-18', 'S-1-5-32-544') { throw "$($rule.IdentityReference) can change $path" }
    }
  }
}
