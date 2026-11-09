param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

foreach ($commandName in @('node', 'npm', 'cargo', 'rustc', 'python')) {
  if ($null -eq (Get-Command $commandName -ErrorAction SilentlyContinue)) {
    throw "Missing source-package prerequisite: $commandName"
  }
}

$rootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$target = ((& rustc -vV) | Select-String '^host: ').Line.Substring(6)
$binary = Join-Path $rootDir 'target\release\cccc.exe'
# A local build never takes the release archive name, which only CI writes. The label
# names the commit, adds -dirty for uncommitted changes, and is "local" outside Git.
# Git's stderr is not redirected: under ErrorActionPreference Stop, Windows PowerShell
# 5.1 can turn redirected stderr output, such as a warning, into a terminating error.
$label = 'local'
if ((Test-Path (Join-Path $rootDir '.git')) -and (Get-Command git -ErrorAction SilentlyContinue)) {
  $sha = & git -C $rootDir rev-parse --short=8 HEAD
  if ($LASTEXITCODE -eq 0 -and $sha) {
    $label = "$sha".Trim()
    $changes = & git -C $rootDir status --porcelain
    if ($LASTEXITCODE -eq 0 -and $changes) {
      $label = "$label-dirty"
    }
  }
}

$prepareArgs = @(
  (Join-Path $rootDir 'scripts\prepare_rust_web_assets.mjs'),
  '--install-deps'
)
& node @prepareArgs
if ($LASTEXITCODE -ne 0) {
  throw "Web asset build failed with exit code $LASTEXITCODE"
}

& cargo build --manifest-path (Join-Path $rootDir 'Cargo.toml') --release --locked --features standalone -p cccc --bin cccc
if ($LASTEXITCODE -ne 0) {
  throw "CCCC build failed with exit code $LASTEXITCODE"
}

& python (Join-Path $rootDir 'scripts\build_standalone_archive.py') $binary --target $target --output-dir (Join-Path $rootDir 'dist') --label $label
if ($LASTEXITCODE -ne 0) {
  throw "Archive build failed with exit code $LASTEXITCODE"
}

& $binary --version
if ($LASTEXITCODE -ne 0) {
  throw "Built CCCC executable failed with exit code $LASTEXITCODE"
}

Write-Host 'OK: built the native CCCC archive in dist/'
Write-Host "Run: $binary"
