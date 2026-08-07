$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$rootDir = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
. (Join-Path $rootDir "scripts\tests\install_windows_concurrency.ps1")
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("cccc-install-test-" + [Guid]::NewGuid().ToString("N"))
$target = "x86_64-pc-windows-msvc"
$binaries = @("cccc.exe")
$versionMatch = Select-String -Path (Join-Path $rootDir "Cargo.toml") -Pattern '^version = "([^"]+)"'
$realVersion = $versionMatch.Matches[0].Groups[1].Value
$releaseBinaryDir = Join-Path $rootDir "target\release"
$originalUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
$originalMachinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
$originalProcessPath = $env:Path
$originalSecurityProtocol = [Net.ServicePointManager]::SecurityProtocol
$originalCcccHome = $env:CCCC_HOME
$originalLauncherPath = $env:CCCC_LAUNCHER_PATH
$machinePathModified = $false
$installerSource = Get-Content -LiteralPath (Join-Path $rootDir "scripts\install.ps1") -Raw
$tlsSnapshot = $installerSource.IndexOf('[Net.ServicePointManager]::SecurityProtocol =')
$downloadSnapshot = $installerSource.IndexOf('Invoke-WebRequest')
if ($tlsSnapshot -lt 0 -or $downloadSnapshot -lt 0 -or $tlsSnapshot -gt $downloadSnapshot) {
  throw "installer does not enable TLS 1.2 before its first HTTPS request"
}
$lockSnapshot = $installerSource.IndexOf('$lockStream = [IO.File]::Open')
$originalSnapshot = $installerSource.IndexOf('$originals += $binary')
if ($lockSnapshot -lt 0 -or $originalSnapshot -lt 0 -or $lockSnapshot -gt $originalSnapshot) {
  throw "installer snapshots existing binaries before acquiring its transaction lock"
}

function Write-ChecksumManifest([string]$ReleaseDir, [string]$Version, [string]$ArchiveChecksum) {
  $entries = @(
    "$("0" * 64)  cccc-v$Version-x86_64-unknown-linux-gnu.tar.gz",
    "$("0" * 64)  cccc-v$Version-x86_64-apple-darwin.tar.gz",
    "$("0" * 64)  cccc-v$Version-aarch64-apple-darwin.tar.gz",
    "$ArchiveChecksum  cccc-v$Version-$target.zip"
  )
  Set-Content -LiteralPath (Join-Path $ReleaseDir "SHA256SUMS") -Value $entries
}

function New-FixtureRelease([string]$Version, [bool]$ValidChecksum, [string]$CcccSource = "") {
  $package = "cccc-v$Version-$target"
  $packageDir = Join-Path $tempRoot "package\$package"
  $releaseDir = Join-Path $tempRoot "releases\download\v$Version"
  New-Item -ItemType Directory -Force -Path $packageDir,$releaseDir | Out-Null
  foreach ($binary in $binaries) {
    $source = if ($CcccSource) { $CcccSource } else { Join-Path $releaseBinaryDir $binary }
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
      throw "Build release binaries before running the Windows installer test: missing $source"
    }
    Copy-Item -LiteralPath $source -Destination (Join-Path $packageDir $binary)
  }
  $archive = Join-Path $releaseDir "$package.zip"
  Compress-Archive -Path $packageDir -DestinationPath $archive
  $archiveChecksum = if ($ValidChecksum) {
    (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
  } else {
    "0" * 64
  }
  Write-ChecksumManifest $releaseDir $Version $archiveChecksum
  Remove-Item -LiteralPath (Join-Path $tempRoot "package") -Recurse -Force
}

function New-UnsafeFixtureRelease([string]$Version, [string]$UnsafeKind) {
  Add-Type -AssemblyName System.IO.Compression
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $package = "cccc-v$Version-$target"
  $releaseDir = Join-Path $tempRoot "releases\download\v$Version"
  New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
  $archive = Join-Path $releaseDir "$package.zip"
  $archiveStream = [IO.File]::Open($archive, [IO.FileMode]::Create)
  $zip = [IO.Compression.ZipArchive]::new($archiveStream, [IO.Compression.ZipArchiveMode]::Create, $false)
  try {
    $binaryEntry = $zip.CreateEntry("$package/cccc.exe")
    $sourceStream = [IO.File]::OpenRead((Join-Path $releaseBinaryDir "cccc.exe"))
    $entryStream = $binaryEntry.Open()
    try {
      $sourceStream.CopyTo($entryStream)
    } finally {
      $entryStream.Dispose()
      $sourceStream.Dispose()
    }
    if ($UnsafeKind -eq "traversal") {
      $unsafeEntry = $zip.CreateEntry("$package/../outside.txt")
    } else {
      $unsafeEntry = $zip.CreateEntry("$package/link")
      $unsafeEntry.ExternalAttributes = -1610612736
    }
    $unsafeStream = $unsafeEntry.Open()
    try {
      $bytes = [Text.Encoding]::UTF8.GetBytes("unsafe")
      $unsafeStream.Write($bytes, 0, $bytes.Length)
    } finally {
      $unsafeStream.Dispose()
    }
  } finally {
    $zip.Dispose()
    $archiveStream.Dispose()
  }
  Write-ChecksumManifest $releaseDir $Version ((Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant())
}

try {
  New-Item -ItemType Directory -Path $tempRoot | Out-Null
  $env:CCCC_HOME = Join-Path $tempRoot "home"
  Remove-Item Env:CCCC_LAUNCHER_PATH -ErrorAction SilentlyContinue
  New-FixtureRelease $realVersion $true
  $installDir = Join-Path $tempRoot "installed"
  $releaseRoot = (Resolve-Path (Join-Path $tempRoot "releases")).Path
  $env:CCCC_RELEASE_BASE_URL = ([Uri]$releaseRoot).AbsoluteUri.TrimEnd('/')
  Remove-Item Env:CCCC_VERSION -ErrorAction SilentlyContinue
  $versionedInstaller = Join-Path $tempRoot "install.ps1"
  (Get-Content -LiteralPath (Join-Path $rootDir "scripts\install.ps1") -Raw).Replace("@CCCC_VERSION@", $realVersion) |
    Set-Content -LiteralPath $versionedInstaller

  $machineCommandDir = Join-Path $tempRoot "machine-cccc"
  New-Item -ItemType Directory -Force -Path $machineCommandDir | Out-Null
  $machineCommand = Join-Path $machineCommandDir "cccc.cmd"
  Set-Content -LiteralPath $machineCommand -Value '@echo cccc 0.2.0' -Encoding Ascii
  $machineCommandHash = (Get-FileHash -LiteralPath $machineCommand).Hash
  $machineConflictInstallDir = Join-Path $tempRoot "machine-conflict-installed"
  try {
    $testMachinePath = if ($originalMachinePath) {
      "$machineCommandDir;$originalMachinePath"
    } else {
      $machineCommandDir
    }
    [Environment]::SetEnvironmentVariable("Path", $testMachinePath, "Machine")
    $machinePathModified = $true
    $failed = $false
    try {
      & (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $machineConflictInstallDir
    } catch {
      $message = $_.Exception.Message
      $isMachineConflict = $message -like "*Machine PATH resolves cccc*"
      $identifiesMachineCommand = $message.IndexOf($machineCommand, [StringComparison]::OrdinalIgnoreCase) -ge 0
      $failed = $isMachineConflict -and $identifiesMachineCommand
    }
    if (-not $failed) { throw "installer did not reject a machine PATH command that wins in new terminals" }
    if (Test-Path -LiteralPath (Join-Path $machineConflictInstallDir "cccc.exe")) {
      throw "installer wrote a binary before rejecting the machine PATH conflict"
    }
    $machineBypassInstallDir = Join-Path $tempRoot "machine-conflict-direct-install"
    & (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $machineBypassInstallDir -NoModifyPath
    $machineBypassVersion = (& (Join-Path $machineBypassInstallDir "cccc.exe") --version | Out-String).Trim()
    if ($machineBypassVersion -ne "cccc $realVersion") {
      throw "-NoModifyPath did not preserve direct installation during a machine PATH conflict"
    }
    if ((Get-FileHash -LiteralPath $machineCommand).Hash -ne $machineCommandHash) {
      throw "installer modified the machine PATH command"
    }
  } finally {
    if ($machinePathModified) {
      [Environment]::SetEnvironmentVariable("Path", $originalMachinePath, "Machine")
      $machinePathModified = $false
    }
  }

  $olderCommandDir = Join-Path $tempRoot "older-cccc"
  New-Item -ItemType Directory -Force -Path $olderCommandDir | Out-Null
  $olderCommand = Join-Path $olderCommandDir "cccc.cmd"
  Set-Content -LiteralPath $olderCommand -Value '@echo cccc 0.3.0' -Encoding Ascii
  $olderCommandHash = (Get-FileHash -LiteralPath $olderCommand).Hash
  $env:Path = "$olderCommandDir;$env:Path"
  $testUserPath = if ($originalUserPath) { "$olderCommandDir;$originalUserPath" } else { $olderCommandDir }
  [Environment]::SetEnvironmentVariable("Path", $testUserPath, "User")

  $foreignInstallDir = Join-Path $tempRoot "foreign-installed"
  New-Item -ItemType Directory -Force -Path $foreignInstallDir | Out-Null
  $foreignCli = Join-Path $foreignInstallDir "cccc.exe"
  Set-Content -LiteralPath $foreignCli -Value "foreign binary" -Encoding Ascii
  Set-Content -LiteralPath (Join-Path $foreignInstallDir ".cccc-standalone") -Value "foreign-v1" -Encoding Ascii
  $foreignHash = (Get-FileHash -LiteralPath $foreignCli).Hash
  $failed = $false
  try {
    & (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $foreignInstallDir -NoModifyPath
  } catch {
    $failed = $_.Exception.Message -like "*managed by another installation; refusing to replace it*"
  }
  if (-not $failed) { throw "installer replaced a command owned by another installation" }
  if ((Get-FileHash -LiteralPath $foreignCli).Hash -ne $foreignHash) {
    throw "installer modified a command owned by another installation"
  }
  if ((Get-Content -LiteralPath (Join-Path $foreignInstallDir ".cccc-standalone") -Raw).Trim() -ne "foreign-v1") {
    throw "installer replaced a foreign ownership marker"
  }

  $versionedInstallDir = Join-Path $tempRoot "versioned-installed"
  & $versionedInstaller -InstallDir $versionedInstallDir -NoModifyPath
  if ((& (Join-Path $versionedInstallDir "cccc.exe") --version | Out-String).Trim() -ne "cccc $realVersion") {
    throw "versioned installer did not use its embedded release version"
  }
  if ((Get-Content -LiteralPath (Join-Path $versionedInstallDir ".cccc-standalone") -Raw).Trim() -ne "standalone-v1") {
    throw "versioned installer did not write its standalone marker"
  }

  & (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $installDir -NoModifyPath

  foreach ($binary in $binaries) {
    $installed = Join-Path $installDir $binary
    if (-not (Test-Path -LiteralPath $installed -PathType Leaf)) { throw "missing installed $binary" }
    if ((Get-FileHash $installed).Hash -ne (Get-FileHash (Join-Path $releaseBinaryDir $binary)).Hash) {
      throw "wrong contents for $binary"
    }
  }
  if ((Get-Content -LiteralPath (Join-Path $installDir ".cccc-standalone") -Raw).Trim() -ne "standalone-v1") {
    throw "installer did not write its standalone marker"
  }
  & (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $installDir -NoModifyPath
  $installOutput = (& (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $installDir *>&1 | Out-String)
  if (-not $installOutput.Contains("Other CCCC commands were left unchanged:")) {
    throw "installer did not report the older CCCC command"
  }
  if (-not $installOutput.Contains("Verify installed command directly:")) {
    throw "installer did not provide an absolute verification command"
  }
  if ($installOutput.IndexOf($olderCommand, [StringComparison]::OrdinalIgnoreCase) -lt 0) {
    throw "installer did not identify the older CCCC command path"
  }
  if ((Get-FileHash -LiteralPath $olderCommand).Hash -ne $olderCommandHash) {
    throw "installer modified an older CCCC command outside its install directory"
  }
  $updatedUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
  $matchingPathEntries = @($updatedUserPath.Split(';', [StringSplitOptions]::RemoveEmptyEntries)).Where({
    $_.TrimEnd('\') -ieq $installDir.TrimEnd('\')
  })
  if ($matchingPathEntries.Count -ne 1) { throw "installer did not add exactly one user PATH entry" }
  if (@($updatedUserPath.Split(';', [StringSplitOptions]::RemoveEmptyEntries)).Where({
      $_.TrimEnd('\') -ieq $olderCommandDir.TrimEnd('\')
    }).Count -ne 1) {
    throw "installer removed or duplicated the older CCCC PATH entry"
  }
  if ($updatedUserPath.Split(';', [StringSplitOptions]::RemoveEmptyEntries)[0].TrimEnd('\') -ine $installDir.TrimEnd('\')) {
    throw "installer did not prepend its user PATH entry"
  }
  $processPathEntries = @($env:Path.Split(';', [StringSplitOptions]::RemoveEmptyEntries))
  if ($processPathEntries[0].TrimEnd('\') -ine $installDir.TrimEnd('\')) {
    throw "installer did not prepend its current-process PATH entry"
  }
  if ($processPathEntries.Where({ $_.TrimEnd('\') -ieq $installDir.TrimEnd('\') }).Count -ne 1) {
    throw "installer duplicated its current-process PATH entry"
  }
  $doctorReport = (& (Join-Path $installDir "cccc.exe") doctor | Out-String | ConvertFrom-Json)
  if ($doctorReport.installation.path_status -ne "ok") {
    throw "Rust doctor did not resolve the installed Windows command"
  }
  if (@($doctorReport.installation.conflicting_commands) -notcontains $olderCommand) {
    throw "Rust doctor did not report the older Windows command"
  }
  & (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $installDir
  $updatedUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
  $matchingPathEntries = @($updatedUserPath.Split(';', [StringSplitOptions]::RemoveEmptyEntries)).Where({
    $_.TrimEnd('\') -ieq $installDir.TrimEnd('\')
  })
  if ($matchingPathEntries.Count -ne 1) { throw "installer duplicated its user PATH entry" }

  $staleParameters = @{
    RootDir = $rootDir
    TempRoot = $tempRoot
    InstallDir = Join-Path $tempRoot "stale-installed"
    RealVersion = $realVersion
    RealBinary = Join-Path $releaseBinaryDir "cccc.exe"
  }
  Test-InstallerConcurrentSnapshot @staleParameters

  $lockPath = Join-Path $installDir ".cccc-install.lock"
  $lockStream = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
  try {
    $failed = $false
    try {
      & (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $installDir -NoModifyPath
    } catch {
      $failed = $_.Exception.Message -like "*Another installation*"
    }
    if (-not $failed) { throw "installer ignored an active transaction lock" }
  } finally {
    $lockStream.Dispose()
    Remove-Item -LiteralPath $lockPath -Force -ErrorAction SilentlyContinue
  }

  foreach ($invalidInstallDir in @("relative-path", "C:\valid;C:\injected")) {
    $failed = $false
    try {
      & (Join-Path $rootDir "scripts\install.ps1") -Version $realVersion -InstallDir $invalidInstallDir -NoModifyPath
    } catch {
      $failed = $_.Exception.Message -like "*absolute path without semicolons*"
    }
    if (-not $failed) { throw "installer accepted invalid InstallDir: $invalidInstallDir" }
  }

  $unsafeIndex = 0
  foreach ($unsafeKind in @("traversal", "link")) {
    $unsafeIndex++
    $unsafeVersion = "0.0.$($unsafeIndex + 10)-test"
    New-UnsafeFixtureRelease $unsafeVersion $unsafeKind
    $failed = $false
    try {
      & (Join-Path $rootDir "scripts\install.ps1") -Version $unsafeVersion -InstallDir (Join-Path $tempRoot "unsafe-$unsafeKind") -NoModifyPath
    } catch {
      $failed = $_.Exception.Message -like "*Archive contains an unsafe path*"
    }
    if (-not $failed) { throw "installer accepted unsafe ZIP entry: $unsafeKind" }
  }

  $badVersion = "0.0.1-test"
  New-FixtureRelease $badVersion $false
  $badInstallDir = Join-Path $tempRoot "bad-installed"
  $failed = $false
  try {
    & (Join-Path $rootDir "scripts\install.ps1") -Version $badVersion -InstallDir $badInstallDir -NoModifyPath
  } catch {
    $failed = $_.Exception.Message -like "*Checksum mismatch*"
  }
  if (-not $failed) { throw "installer accepted a bad checksum" }
  if (Test-Path -LiteralPath (Join-Path $badInstallDir "cccc.exe")) { throw "bad download was installed" }

  $hashesBefore = @{}
  foreach ($binary in $binaries) {
    $hashesBefore[$binary] = (Get-FileHash (Join-Path $installDir $binary)).Hash
  }
  $mismatchVersion = "9.9.9"
  New-FixtureRelease $mismatchVersion $true
  $failed = $false
  try {
    & (Join-Path $rootDir "scripts\install.ps1") -Version $mismatchVersion -InstallDir $installDir -NoModifyPath
  } catch {
    $failed = $_.Exception.Message -like "*Installed version mismatch*"
  }
  if (-not $failed) { throw "installer accepted a mismatched binary version" }
  foreach ($binary in $binaries) {
    if ((Get-FileHash (Join-Path $installDir $binary)).Hash -ne $hashesBefore[$binary]) {
      throw "rollback did not restore $binary"
    }
  }

  $readonlyMarkerVersion = "9.9.5"
  $readonlyMarkerSource = Join-Path $tempRoot "readonly-marker-version.rs"
  $readonlyMarkerBinary = Join-Path $tempRoot "readonly-marker-version.exe"
  Set-Content -LiteralPath $readonlyMarkerSource -Encoding utf8 -Value @'
use std::env;
fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && args[0] == "--version" {
        println!("cccc 9.9.5");
        return;
    }
    if args.len() == 2 && args[0] == "daemon" && args[1] == "start" {
        return;
    }
    std::process::exit(1);
}
'@
  & rustc $readonlyMarkerSource -O -o $readonlyMarkerBinary
  if ($LASTEXITCODE -ne 0) { throw "failed to build read-only marker fixture" }
  New-FixtureRelease $readonlyMarkerVersion $true $readonlyMarkerBinary
  $readonlyMarkerInstall = Join-Path $tempRoot "readonly-marker-installed"
  $readonlyMarkerHome = Join-Path $tempRoot "readonly-marker-home"
  New-Item -ItemType Directory -Force -Path $readonlyMarkerInstall | Out-Null
  Copy-Item -LiteralPath (Join-Path $releaseBinaryDir "cccc.exe") -Destination (Join-Path $readonlyMarkerInstall "cccc.exe")
  $readonlyMarkerPath = Join-Path $readonlyMarkerInstall ".cccc-standalone"
  Set-Content -LiteralPath $readonlyMarkerPath -Value "foreign-v1" -Encoding Ascii
  (Get-Item -LiteralPath $readonlyMarkerPath).IsReadOnly = $true
  $homeBeforeReadonlyMarker = $env:CCCC_HOME
  $allowReplaceBeforeReadonlyMarker = $env:CCCC_ALLOW_REPLACE_EXISTING
  try {
    $env:CCCC_HOME = $readonlyMarkerHome
    $env:CCCC_ALLOW_REPLACE_EXISTING = "1"
    & (Join-Path $readonlyMarkerInstall "cccc.exe") daemon start
    if ($LASTEXITCODE -ne 0) { throw "failed to start the read-only marker fixture daemon" }
    & (Join-Path $rootDir "scripts\install.ps1") -Version $readonlyMarkerVersion -InstallDir $readonlyMarkerInstall -NoModifyPath
  } finally {
    $env:CCCC_HOME = $homeBeforeReadonlyMarker
    if ($null -eq $allowReplaceBeforeReadonlyMarker) {
      Remove-Item Env:CCCC_ALLOW_REPLACE_EXISTING -ErrorAction SilentlyContinue
    } else {
      $env:CCCC_ALLOW_REPLACE_EXISTING = $allowReplaceBeforeReadonlyMarker
    }
  }
  $readonlyMarkerReported = (& (Join-Path $readonlyMarkerInstall "cccc.exe") --version | Out-String).Trim()
  if ($readonlyMarkerReported -ne "cccc $readonlyMarkerVersion") {
    throw "installer did not replace the read-only foreign marker fixture"
  }
  if ((Get-Content -LiteralPath $readonlyMarkerPath -Raw).Trim() -ne "standalone-v1") {
    throw "installer did not atomically replace the read-only ownership marker"
  }

  $slowSource = Join-Path $tempRoot "slow-version.rs"
  $slowBinary = Join-Path $tempRoot "slow-version.exe"
  Set-Content -LiteralPath $slowSource -Encoding utf8 -Value @'
use std::{env, thread, time::Duration};
fn main() {
    if env::args().any(|arg| arg == "--version") {
        println!("cccc 9.9.8");
        thread::sleep(Duration::from_secs(5));
    }
}
'@
  & rustc $slowSource -O -o $slowBinary
  if ($LASTEXITCODE -ne 0) { throw "failed to build locked rollback fixture" }
  $lockedVersion = "9.9.6"
  New-FixtureRelease $lockedVersion $true $slowBinary
  $oldHash = (Get-FileHash (Join-Path $installDir "cccc.exe")).Hash
  $childOut = Join-Path $tempRoot "locked-rollback.out"
  $childErr = Join-Path $tempRoot "locked-rollback.err"
  $hostExecutable = (Get-Process -Id $PID).Path
  $childArguments = @(
    "-NoProfile", "-File", (Join-Path $rootDir "scripts\install.ps1"),
    "-Version", $lockedVersion, "-InstallDir", $installDir, "-NoModifyPath"
  )
  $child = Start-Process -FilePath $hostExecutable -PassThru -NoNewWindow -ArgumentList $childArguments -RedirectStandardOutput $childOut -RedirectStandardError $childErr
  $heldBinary = $null
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  while (-not $child.HasExited -and [DateTime]::UtcNow -lt $deadline -and $null -eq $heldBinary) {
    try {
      $currentHash = (Get-FileHash (Join-Path $installDir "cccc.exe") -ErrorAction Stop).Hash
      if ($currentHash -ne $oldHash) {
        $heldBinary = [IO.File]::Open(
          (Join-Path $installDir "cccc.exe"),
          [IO.FileMode]::Open,
          [IO.FileAccess]::Read,
          [IO.FileShare]::Read
        )
      }
    } catch {}
    # Get-FileHash does not share the file for deletion on Windows. Leave a
    # window for the installer to rename the old executable between probes.
    if ($null -eq $heldBinary) { Start-Sleep -Milliseconds 10 }
  }
  if ($null -eq $heldBinary) {
    if (-not $child.HasExited) {
      $child.Kill()
      $child.WaitForExit()
    }
    $childLogs = @(
      Get-Content -LiteralPath $childOut -Raw -ErrorAction SilentlyContinue
      Get-Content -LiteralPath $childErr -Raw -ErrorAction SilentlyContinue
    ) -join [Environment]::NewLine
    throw "failed to acquire the replacement binary lock. Child output:`n$childLogs"
  }
  $child.WaitForExit()
  $backupDir = Join-Path $installDir (".cccc-backup-" + $child.Id)
  $backupBinary = Join-Path $backupDir "cccc.exe"
  if ($child.ExitCode -eq 0) { throw "locked rollback fixture unexpectedly succeeded" }
  if (-not (Test-Path -LiteralPath $backupBinary -PathType Leaf)) { throw "rollback deleted its only old binary backup" }
  if ((Get-FileHash $backupBinary).Hash -ne $oldHash) { throw "retained rollback backup has the wrong bytes" }
  $diagnostic = Get-Content -LiteralPath $childErr -Raw
  if ($diagnostic -notlike "*Rollback failed to restore*" -or
      $diagnostic -notlike "*Previous binary backup retained at $backupDir*") {
    throw "rollback did not report its retained backup"
  }
  $probe = [IO.File]::Open(
    (Join-Path $installDir ".cccc-install.lock"),
    [IO.FileMode]::OpenOrCreate,
    [IO.FileAccess]::ReadWrite,
    [IO.FileShare]::None
  )
  $probe.Dispose()
  Remove-Item -LiteralPath (Join-Path $installDir ".cccc-install.lock") -Force -ErrorAction SilentlyContinue
  $heldBinary.Dispose()
  Remove-Item -LiteralPath (Join-Path $installDir "cccc.exe") -Force
  Move-Item -LiteralPath $backupBinary -Destination (Join-Path $installDir "cccc.exe")
  Remove-Item -LiteralPath $backupDir -Recurse -Force

  Write-Host "OK: Windows installer"
} finally {
  if ($machinePathModified) {
    [Environment]::SetEnvironmentVariable("Path", $originalMachinePath, "Machine")
  }
  [Environment]::SetEnvironmentVariable("Path", $originalUserPath, "User")
  $env:Path = $originalProcessPath
  [Net.ServicePointManager]::SecurityProtocol = $originalSecurityProtocol
  if ($null -eq $originalCcccHome) { Remove-Item Env:CCCC_HOME -ErrorAction SilentlyContinue } else { $env:CCCC_HOME = $originalCcccHome }
  if ($null -eq $originalLauncherPath) { Remove-Item Env:CCCC_LAUNCHER_PATH -ErrorAction SilentlyContinue } else { $env:CCCC_LAUNCHER_PATH = $originalLauncherPath }
  Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
