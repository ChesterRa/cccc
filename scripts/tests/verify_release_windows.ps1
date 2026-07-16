[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$ArtifactDir,
  [Parameter(Mandatory = $true)][string]$Target
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$artifactDir = (Resolve-Path $ArtifactDir).Path
$versionLine = Select-String -Path (Join-Path $rootDir "Cargo.toml") -Pattern '^version = "([^"]+)"$' | Select-Object -First 1
$version = $versionLine.Matches[0].Groups[1].Value
if ($Target -ne "x86_64-pc-windows-msvc") { throw "unexpected Windows target: $Target" }

$package = "cccc-v$version-$Target"
$archive = Join-Path $artifactDir "$package.zip"
$installer = Join-Path $artifactDir "install.ps1"
foreach ($required in @($archive, (Join-Path $artifactDir "SHA256SUMS"), $installer)) {
  if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "missing release asset: $required" }
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("cccc-release-verify-" + [Guid]::NewGuid().ToString("N"))
$installed = Join-Path $tempRoot "installed\cccc.exe"
$home = Join-Path $tempRoot "home"
try {
  $extractRoot = Join-Path $tempRoot "extracted"
  New-Item -ItemType Directory -Path $extractRoot | Out-Null
  Expand-Archive -LiteralPath $archive -DestinationPath $extractRoot
  $packageDir = Join-Path $extractRoot $package
  $packageBinary = Join-Path $packageDir "cccc.exe"
  if (-not (Test-Path -LiteralPath $packageBinary -PathType Leaf)) { throw "archive is missing cccc.exe" }
  $executables = @(Get-ChildItem -LiteralPath $packageDir -File -Filter "*.exe")
  if ($executables.Count -ne 1 -or $executables[0].Name -ne "cccc.exe") {
    throw "archive must contain exactly one executable: cccc.exe"
  }

  $releaseDir = Join-Path $tempRoot "releases\download\v$version"
  New-Item -ItemType Directory -Path $releaseDir | Out-Null
  Copy-Item -Path (Join-Path $artifactDir "*") -Destination $releaseDir
  $releaseBaseUrl = ([Uri]::new((Resolve-Path (Join-Path $tempRoot "releases")).Path)).AbsoluteUri.TrimEnd("/")

  $env:CCCC_VERSION = $version
  $env:CCCC_RELEASE_BASE_URL = $releaseBaseUrl
  $env:CCCC_INSTALL_DIR = Join-Path $tempRoot "installed"
  $env:CCCC_NO_MODIFY_PATH = "1"
  & $installer -NoModifyPath

  if (-not (Test-Path -LiteralPath $installed -PathType Leaf)) { throw "installer did not install cccc.exe" }
  $installedFiles = @(Get-ChildItem -LiteralPath (Split-Path $installed) -File)
  if ($installedFiles.Count -ne 1 -or $installedFiles[0].Name -ne "cccc.exe") {
    throw "installer must install exactly one executable: cccc.exe"
  }
  if ((Get-FileHash $installed).Hash -ne (Get-FileHash $packageBinary).Hash) {
    throw "installed cccc.exe differs from the release archive"
  }
  $reportedVersion = (& $installed --version | Out-String).Trim()
  if ($LASTEXITCODE -ne 0 -or $reportedVersion -ne "cccc $version") {
    throw "installed version mismatch: $reportedVersion"
  }

  $env:CCCC_HOME = $home
  & $installed daemon start
  if ($LASTEXITCODE -ne 0) { throw "daemon start failed" }
  & $installed daemon status
  if ($LASTEXITCODE -ne 0) { throw "daemon status failed" }
  & $installed daemon stop
  if ($LASTEXITCODE -ne 0) { throw "daemon stop failed" }

  $address = Join-Path $home "daemon\ccccd.addr.json"
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  while ((Test-Path -LiteralPath $address) -and [DateTime]::UtcNow -lt $deadline) {
    Start-Sleep -Milliseconds 100
  }
  if (Test-Path -LiteralPath $address) { throw "daemon did not remove $address" }

  $stoppedBinary = "$installed.stopped"
  $released = $false
  for ($attempt = 0; $attempt -lt 40; $attempt++) {
    try {
      Move-Item -LiteralPath $installed -Destination $stoppedBinary
      Move-Item -LiteralPath $stoppedBinary -Destination $installed
      $released = $true
      break
    } catch {
      if ((Test-Path -LiteralPath $stoppedBinary) -and -not (Test-Path -LiteralPath $installed)) {
        Move-Item -LiteralPath $stoppedBinary -Destination $installed -ErrorAction SilentlyContinue
      }
      Start-Sleep -Milliseconds 250
    }
  }
  if (-not $released) { throw "daemon did not release the installed cccc.exe" }

  Write-Host "OK: verified $package release archive and installed self-launch"
} finally {
  if (Test-Path -LiteralPath $installed -PathType Leaf) {
    $env:CCCC_HOME = $home
    & $installed daemon stop *> $null
  }
  Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
