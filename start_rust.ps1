param(
  [string]$WebHost = '127.0.0.1',
  [int]$WebPort = 8848,
  [switch]$LocalHome,
  [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

function Require-Command([string]$Name) {
  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "Missing required command: $Name"
  }
}

Require-Command 'cargo'
if (-not $SkipBuild) {
  Require-Command 'npm'
  npm ci --prefix (Join-Path $repoRoot 'web') | Out-Host
  npm -C (Join-Path $repoRoot 'web') run build | Out-Host
  cargo build --workspace --bins --locked | Out-Host
}

if ($LocalHome) {
  $env:CCCC_HOME = (Join-Path $repoRoot '.cccc-rust')
}

$env:CCCC_WEB_HOST = $WebHost
$env:CCCC_WEB_PORT = "$WebPort"
$binary = Join-Path $repoRoot 'target\debug\cccc.exe'
if (-not (Test-Path $binary)) {
  throw "Missing $binary; run without -SkipBuild first."
}

Write-Host "[start] Rust CCCC: http://${WebHost}:${WebPort}/"
& $binary
