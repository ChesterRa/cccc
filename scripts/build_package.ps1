param(
  [switch]$InstallDeps,
  [string]$Python = "python"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$rootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

& (Join-Path $rootDir "scripts\build_web.ps1") -InstallDeps:$InstallDeps

& $Python -m pip install -U pip build twine | Out-Host
& $Python -m compileall -q (Join-Path $rootDir "src\cccc") | Out-Host
& $Python -m build $rootDir | Out-Host
& $Python -m twine check (Join-Path $rootDir "dist\*") | Out-Host

Write-Host "OK: 已构建 dist/*，并打包 bundled Web UI"
