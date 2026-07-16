param(
  [string]$Python = '3.11',
  [string]$WebHost = '127.0.0.1',
  [int]$WebPort = 8848,
  [ValidateSet('info', 'debug', 'warning', 'error', 'critical')]
  [string]$WebLogLevel = 'info',
  [switch]$Reload,
  [switch]$LocalHome,
  [switch]$SkipInstall
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

function Require-Command([string]$Name) {
  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "Missing required command: $Name. Install it first (https://github.com/astral-sh/uv)."
  }
}

Require-Command 'uv'

$venvDir = Join-Path $repoRoot '.venv'
$venvPython = Join-Path $venvDir 'Scripts\python.exe'

if (-not (Test-Path $venvPython)) {
  Write-Host "[start] Creating venv (.venv) with Python $Python..."
  uv venv -p $Python $venvDir | Out-Host
}

if (-not $SkipInstall) {
  Write-Host "[start] Installing (editable) with uv..."
  uv pip install -p $venvPython -e . | Out-Host
}

# Optional: isolate runtime home to this repo (kept in .gitignore).
if ($LocalHome) {
  $env:CCCC_HOME = (Join-Path $repoRoot '.cccc')
}

$env:CCCC_WEB_HOST = $WebHost
$env:CCCC_WEB_PORT = "$WebPort"
$env:CCCC_WEB_LOG_LEVEL = $WebLogLevel
if ($Reload) { $env:CCCC_WEB_RELOAD = '1' } else { if ($env:CCCC_WEB_RELOAD) { Remove-Item Env:CCCC_WEB_RELOAD -ErrorAction SilentlyContinue } }

# Port preflight (helpful when a stale web process is already running).
try {
  $listeners = Get-NetTCPConnection -State Listen -LocalPort $WebPort -ErrorAction SilentlyContinue
  if ($listeners) {
    try {
      $ping = Invoke-RestMethod -TimeoutSec 2 -Uri ("http://{0}:{1}/api/v1/ping" -f $WebHost, $WebPort)
      if ($ping -and $ping.ok -eq $true) {
        Write-Host ("[start] Already running: http://{0}:{1}/ui/" -f $WebHost, $WebPort)
        if ($LocalHome -and $ping.result -and $ping.result.home) {
          Write-Host ("[start] Server home: {0}" -f $ping.result.home)
        }
        return
      }
    } catch {
      # Ignore; fall through and report the port conflict.
    }

    $pids = ($listeners | Select-Object -ExpandProperty OwningProcess -Unique) -join ','
    throw "Port $WebPort is already in use (PID(s): $pids). Use -WebPort <port> or stop the existing process."
  }
} catch {
  # Get-NetTCPConnection may not exist on some minimal environments; ignore.
}

Write-Host "[start] Starting CCCC (daemon + web)..."
Write-Host ("[start] UI: http://{0}:{1}/ui/" -f $WebHost, $WebPort)
Write-Host "[start] Press Ctrl+C to stop."

uv run -p $venvPython --no-sync cccc
