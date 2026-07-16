param(
  [switch]$InstallDeps
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$rootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$webDir = Join-Path $rootDir "web"
$distIndex = Join-Path $rootDir "web\dist\index.html"
$pythonDist = Join-Path $rootDir "src\cccc\ports\web\dist"

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
  throw "缺少 npm，请先安装 Node.js。"
}

if ($InstallDeps) {
  npm ci --prefix $webDir | Out-Host
}

npm -C $webDir run build | Out-Host

if (-not (Test-Path $distIndex)) {
  throw "Web 构建失败，未找到 $distIndex"
}

Remove-Item -Recurse -Force $pythonDist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $pythonDist | Out-Null
Copy-Item -Recurse (Join-Path $webDir "dist\*") $pythonDist
Write-Host "OK: 已构建 bundled Web UI -> web/dist 和 src/cccc/ports/web/dist"
