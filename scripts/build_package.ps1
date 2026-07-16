param([switch]$InstallDeps)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$rootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$version = (Select-String -Path (Join-Path $rootDir "Cargo.toml") -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
$target = ((& rustc -vV) | Select-String '^host: ').Line.Substring(6)
$name = "cccc-v$version-$target"

& (Join-Path $rootDir "scripts\build_web.ps1") -InstallDeps:$InstallDeps
& cargo build --manifest-path (Join-Path $rootDir "Cargo.toml") --release --locked -p cccc-cli --bin cccc

$output = Join-Path $rootDir "dist\$name"
Remove-Item -Recurse -Force $output -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $output | Out-Null
Copy-Item (Join-Path $rootDir "target\release\cccc.exe") $output
Copy-Item (Join-Path $rootDir "LICENSE"),(Join-Path $rootDir "README.md"),(Join-Path $rootDir "docs\rust-migration.md") $output
Compress-Archive -Force -Path $output -DestinationPath (Join-Path $rootDir "dist\$name.zip")
Write-Host "OK: built dist/$name.zip"
