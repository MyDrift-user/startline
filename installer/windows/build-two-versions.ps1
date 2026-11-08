# For the update test: builds the release files twice with the test key, as this version into old\ and as 99.0.0 into
# www\, with a signed list next to them. Needs VERSION, Inno Setup and WiX.
$ErrorActionPreference = 'Stop'
$version = $env:VERSION

cargo build --release --locked --features update-test
if ($LASTEXITCODE) { throw "cargo build failed" }
& "$PSScriptRoot\package.ps1"
Move-Item dist old

$toml = Get-Content Cargo.toml -Raw
Set-Content Cargo.toml ([regex]::new('(?m)^version = ".*"$').Replace($toml, 'version = "99.0.0"', 1)) -NoNewline
try {
  cargo build --release --features update-test
  if ($LASTEXITCODE) { throw "cargo build failed" }
  $env:VERSION = '99.0.0'
  & "$PSScriptRoot\package.ps1"
} finally {
  $env:VERSION = $version
  git checkout -- Cargo.toml Cargo.lock
}
Move-Item dist www

$list = "version 99.0.0`n"
foreach ($file in Get-ChildItem www -File) {
  $list += "file $((Get-FileHash $file.FullName -Algorithm SHA256).Hash.ToLower()) $($file.Name)`n"
}
[IO.File]::WriteAllText("$PWD\www\latest.json", $list)
# A windowed program, so wait for it explicitly.
$p = Start-Process "old\startline-v$version-windows-x64-portable.exe" -ArgumentList '--test-sign', "`"$PWD\www\latest.json`"", "`"$PWD\www\latest.json.sig`"" -Wait -PassThru
if ($p.ExitCode -or -not (Test-Path www\latest.json.sig)) { throw "Signing the test list failed" }
