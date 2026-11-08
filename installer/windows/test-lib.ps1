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
