[CmdletBinding()]
param(
  [string]$Version = $env:CCCC_VERSION,
  [string]$InstallDir = $env:CCCC_INSTALL_DIR,
  [switch]$NoModifyPath
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# Windows PowerShell 5.1 can otherwise negotiate an obsolete TLS protocol.
[Net.ServicePointManager]::SecurityProtocol =
  [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$defaultVersion = "@CCCC_VERSION@"
$releaseTagPrefix = if ($env:CCCC_RELEASE_TAG_PREFIX) { $env:CCCC_RELEASE_TAG_PREFIX } else { "@CCCC_RELEASE_TAG_PREFIX@" }
if ($releaseTagPrefix.StartsWith("@")) {
  $releaseTagPrefix = "v"
}
$repository = if ($env:CCCC_GITHUB_REPOSITORY) { $env:CCCC_GITHUB_REPOSITORY } else { "ChesterRa/cccc" }
$releaseBaseUrl = if ($env:CCCC_RELEASE_BASE_URL) {
  $env:CCCC_RELEASE_BASE_URL.TrimEnd("/")
} else {
  "https://github.com/$repository/releases"
}
$NoModifyPath = $NoModifyPath -or $env:CCCC_NO_MODIFY_PATH -eq "1"
$allowReplaceExisting = $env:CCCC_ALLOW_REPLACE_EXISTING -eq "1"
$installMarker = ".cccc-standalone"
$installMarkerVersion = "standalone-v1"
if (-not $InstallDir) {
  $InstallDir = Join-Path $env:LOCALAPPDATA "CCCC\bin"
}
if ([string]::IsNullOrWhiteSpace($InstallDir) -or $InstallDir.Contains(';') -or
    -not [IO.Path]::IsPathRooted($InstallDir)) {
  throw "InstallDir must be an absolute path without semicolons: $InstallDir"
}
$InstallDir = [IO.Path]::GetFullPath($InstallDir)

function Get-ResponseUri([object]$Response) {
  if ($Response.BaseResponse.PSObject.Properties.Name -contains "ResponseUri") {
    return $Response.BaseResponse.ResponseUri.AbsoluteUri
  }
  if ($Response.BaseResponse.PSObject.Properties.Name -contains "RequestMessage") {
    return $Response.BaseResponse.RequestMessage.RequestUri.AbsoluteUri
  }
  throw "Could not resolve the latest release URI"
}

function Get-CcccCommandPaths([string]$PathValue) {
  if ([string]::IsNullOrWhiteSpace($PathValue)) { return }
  $extensions = if ($env:PATHEXT) { @($env:PATHEXT.Split(';')) } else { @('.COM', '.EXE', '.BAT', '.CMD') }
  $names = @('cccc')
  foreach ($extension in $extensions) {
    $extension = $extension.Trim()
    if (-not $extension) { continue }
    if (-not $extension.StartsWith('.')) { $extension = ".$extension" }
    $names += "cccc$extension"
  }
  if ($names -notcontains 'cccc.ps1') { $names += 'cccc.ps1' }
  $seen = @{}
  foreach ($entry in $PathValue.Split(';', [StringSplitOptions]::RemoveEmptyEntries)) {
    $directory = [Environment]::ExpandEnvironmentVariables($entry.Trim().Trim('"'))
    if (-not $directory) { continue }
    foreach ($name in $names) {
      $candidate = Join-Path $directory $name
      if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { continue }
      $fullPath = [IO.Path]::GetFullPath($candidate)
      $key = $fullPath.ToUpperInvariant()
      if ($seen.ContainsKey($key)) { continue }
      $seen[$key] = $true
      Write-Output $fullPath
    }
  }
}

function Test-SameCommandPath([string]$Left, [string]$Right) {
  if (-not $Left -or -not $Right) { return $false }
  return [IO.Path]::GetFullPath($Left).Equals(
    [IO.Path]::GetFullPath($Right),
    [StringComparison]::OrdinalIgnoreCase
  )
}

function Test-SameDirectoryPath([string]$Entry, [string]$Directory) {
  if ([string]::IsNullOrWhiteSpace($Entry) -or [string]::IsNullOrWhiteSpace($Directory)) {
    return $false
  }
  try {
    $expanded = [Environment]::ExpandEnvironmentVariables($Entry.Trim().Trim('"'))
    return [IO.Path]::GetFullPath($expanded).TrimEnd('\').Equals(
      [IO.Path]::GetFullPath($Directory).TrimEnd('\'),
      [StringComparison]::OrdinalIgnoreCase
    )
  } catch {
    return $false
  }
}

function Add-DirectoryToPathFront([string]$PathValue, [string]$Directory) {
  $entries = if ($PathValue) {
    @($PathValue.Split(';', [StringSplitOptions]::RemoveEmptyEntries))
  } else {
    @()
  }
  $remaining = @($entries.Where({ -not (Test-SameDirectoryPath $_ $Directory) }))
  return (@($Directory) + $remaining) -join ';'
}

function Move-CcccItemWithRetry([string]$Source, [string]$Destination) {
  for ($attempt = 0; $attempt -lt 40; $attempt++) {
    try {
      Move-Item -LiteralPath $Source -Destination $Destination -ErrorAction Stop
      return
    } catch {
      if ($attempt -eq 39) { throw }
      Start-Sleep -Milliseconds 50
    }
  }
}

function Join-PersistedWindowsPath([string]$MachinePath, [string]$UserPath) {
  $parts = @()
  if (-not [string]::IsNullOrWhiteSpace($MachinePath)) { $parts += $MachinePath }
  if (-not [string]::IsNullOrWhiteSpace($UserPath)) { $parts += $UserPath }
  return $parts -join ';'
}

function Get-ProspectiveCcccCommandPath([string]$PathValue, [string]$InstalledCommand) {
  if ([string]::IsNullOrWhiteSpace($PathValue)) { return $null }
  $installDirectory = Split-Path -Parent $InstalledCommand
  foreach ($entry in $PathValue.Split(';', [StringSplitOptions]::RemoveEmptyEntries)) {
    if (Test-SameDirectoryPath $entry $installDirectory) {
      return [IO.Path]::GetFullPath($InstalledCommand)
    }
    $commands = @(Get-CcccCommandPaths $entry)
    if ($commands.Count -gt 0) { return $commands[0] }
  }
  return $null
}

function Receive-File([string]$Uri, [string]$Destination) {
  $parsed = [Uri]$Uri
  if ($parsed.IsFile) {
    Copy-Item -LiteralPath $parsed.LocalPath -Destination $Destination
    return
  }
  $response = Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $Destination -PassThru
  if (-not $env:CCCC_RELEASE_BASE_URL) {
    $effectiveUri = [Uri](Get-ResponseUri $response)
    $trustedHost = $effectiveUri.Host -eq "github.com" -or $effectiveUri.Host.EndsWith(".githubusercontent.com")
    if ($effectiveUri.Scheme -ne "https" -or -not $trustedHost) {
      throw "Release asset redirected outside GitHub HTTPS: $effectiveUri"
    }
  }
}

$architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$isWindowsRuntime = if ($PSVersionTable.PSEdition -eq "Core") { $IsWindows } else { $true }
if (-not $isWindowsRuntime) {
  throw "This installer is for Windows. Use install.sh on macOS or Linux."
}
if ($architecture -ne "X64") {
  throw "Unsupported Windows architecture: $architecture"
}

$installedCommand = [IO.Path]::GetFullPath((Join-Path $InstallDir "cccc.exe"))
if (-not $NoModifyPath) {
  $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  $prospectiveUserPath = Add-DirectoryToPathFront $userPath $InstallDir
  $prospectivePath = Join-PersistedWindowsPath $machinePath $prospectiveUserPath
  $prospectiveCommand = Get-ProspectiveCcccCommandPath $prospectivePath $installedCommand
  if (-not (Test-SameCommandPath $prospectiveCommand $installedCommand)) {
    throw "Machine PATH resolves cccc to $prospectiveCommand before the per-user install directory $InstallDir. Windows places Machine PATH before User PATH in a new terminal, so this installer cannot safely override that command without changing machine-wide state. Remove or upgrade the machine-wide CCCC from an elevated shell, or rerun with -NoModifyPath and invoke $installedCommand directly"
  }
}

if (-not $Version -and $defaultVersion -match '^[0-9]+\.[0-9]+\.[0-9]+') {
  $Version = $defaultVersion
}
if (-not $Version) {
  $latest = Invoke-WebRequest -UseBasicParsing -Uri "$releaseBaseUrl/latest"
  $latestUri = Get-ResponseUri $latest
  if (-not $env:CCCC_RELEASE_BASE_URL) {
    $expectedPrefix = "https://github.com/$repository/releases/tag/v"
    if (-not $latestUri.StartsWith($expectedPrefix, [StringComparison]::Ordinal)) {
      throw "Latest release redirected outside $expectedPrefix"
    }
  }
  $tag = $latestUri.TrimEnd("/").Split("/")[-1]
  if (-not $tag.StartsWith("v")) {
    throw "Latest release did not resolve to a v-prefixed tag: $tag"
  }
  $Version = $tag.Substring(1)
} else {
  $Version = $Version.TrimStart("v")
}

if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?(\+[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$') {
  throw "Invalid semantic version: $Version"
}

$target = "x86_64-pc-windows-msvc"
$package = "cccc-v$Version-$target"
$archive = "$package.zip"
$downloadUrl = "$releaseBaseUrl/download/$releaseTagPrefix$Version"
$tempDir = Join-Path ([IO.Path]::GetTempPath()) ("cccc-install-" + [Guid]::NewGuid().ToString("N"))
$binaries = @("cccc.exe")
$staged = @()
$originals = @()
$backupDir = Join-Path $InstallDir (".cccc-backup-" + $PID)
$lockPath = Join-Path $InstallDir ".cccc-install.lock"
$lockStream = $null
$transactionStarted = $false
$transactionCommitted = $false
$daemonWasRunning = $false
$rollbackRestoreFailed = $false
$pathModified = $false
$userPathBeforeInstall = $null
$processPathBeforeInstall = $env:Path
$markerPath = Join-Path $InstallDir $installMarker
$markerStage = "$markerPath.cccc-install-$PID"
$markerBackup = Join-Path $backupDir $installMarker
$markerTouched = $false
$markerOriginal = $false

try {
  New-Item -ItemType Directory -Path $tempDir | Out-Null
  Write-Host "Downloading CCCC v$Version for $target..."
  $archivePath = Join-Path $tempDir $archive
  $checksumsPath = Join-Path $tempDir "SHA256SUMS"
  Receive-File "$downloadUrl/SHA256SUMS" $checksumsPath

  $expectedArchives = @(
    "cccc-v$Version-x86_64-unknown-linux-gnu.tar.gz",
    "cccc-v$Version-x86_64-apple-darwin.tar.gz",
    "cccc-v$Version-aarch64-apple-darwin.tar.gz",
    "cccc-v$Version-x86_64-pc-windows-msvc.zip"
  )
  $checksumEntries = @{}
  foreach ($line in Get-Content -LiteralPath $checksumsPath) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    if ($line -notmatch '^([0-9A-Fa-f]{64})[ \t]+\*?([^/\\]+)$') {
      throw "SHA256SUMS must contain four unique, well-formed archive entries"
    }
    $name = $Matches[2]
    if ($expectedArchives -notcontains $name -or $checksumEntries.ContainsKey($name)) {
      throw "SHA256SUMS must contain four unique, well-formed archive entries"
    }
    $checksumEntries[$name] = $Matches[1].ToLowerInvariant()
  }
  if ($checksumEntries.Count -ne 4 -or -not $checksumEntries.ContainsKey($archive)) {
    throw "SHA256SUMS must contain exactly one entry for $archive and four entries total"
  }

  Receive-File "$downloadUrl/$archive" $archivePath
  $actualChecksum = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
  if ($actualChecksum -ne $checksumEntries[$archive]) {
    throw "Checksum mismatch for $archive"
  }

  Add-Type -AssemblyName System.IO.Compression
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $zip = [IO.Compression.ZipFile]::OpenRead($archivePath)
  try {
    $canonicalTempRoot = [IO.Path]::GetFullPath($tempDir + [IO.Path]::DirectorySeparatorChar)
    foreach ($entry in $zip.Entries) {
      $entryPath = $entry.FullName.Replace('\', '/')
      $insidePackage = $entryPath -eq "$package/" -or $entryPath.StartsWith("$package/", [StringComparison]::Ordinal)
      if ($entryPath.StartsWith('/') -or $entryPath -match '^[A-Za-z]:' -or
          $entryPath -match '(^|/)\.\.(/|$)' -or -not $insidePackage) {
        throw "Archive contains an unsafe path: $entryPath"
      }
      $canonicalDestination = [IO.Path]::GetFullPath((Join-Path $tempDir $entryPath))
      $unixType = ($entry.ExternalAttributes -shr 16) -band 0xF000
      $dosAttributes = $entry.ExternalAttributes -band 0xFFFF
      $supportedType = $unixType -eq 0 -or $unixType -eq 0x4000 -or $unixType -eq 0x8000
      $isReparsePoint = ($dosAttributes -band [int][IO.FileAttributes]::ReparsePoint) -ne 0
      if (-not $canonicalDestination.StartsWith($canonicalTempRoot, [StringComparison]::OrdinalIgnoreCase) -or
          -not $supportedType -or $isReparsePoint) {
        throw "Archive contains an unsafe path: $entryPath"
      }
    }
  } finally {
    $zip.Dispose()
  }
  Expand-Archive -LiteralPath $archivePath -DestinationPath $tempDir
  $packageDir = Join-Path $tempDir $package
  if (-not (Test-Path -LiteralPath $packageDir -PathType Container)) {
    throw "Archive is missing its package directory"
  }

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  foreach ($binary in $binaries) {
    $source = Join-Path $packageDir $binary
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
      throw "Archive is missing $binary"
    }
    $stage = Join-Path $InstallDir (".$binary.cccc-install-" + $PID)
    Copy-Item -LiteralPath $source -Destination $stage -Force
    $staged += $stage
  }

  try {
    $lockStream = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $lockStream.SetLength(0)
    $lockBytes = [Text.Encoding]::UTF8.GetBytes("$PID`n")
    $lockStream.Write($lockBytes, 0, $lockBytes.Length)
    $lockStream.Flush()
  } catch {
    throw "Another installation is using $InstallDir (lock: $lockPath)"
  }

  $existingCli = Join-Path $InstallDir "cccc.exe"
  $ownedByStandaloneInstaller = $false
  if (Test-Path -LiteralPath $markerPath) {
    $markerItem = Get-Item -LiteralPath $markerPath -Force
    $markerIsRegularFile = -not $markerItem.PSIsContainer -and
      ($markerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0
    if (-not $markerIsRegularFile) {
      throw "Existing standalone ownership marker is not a regular file: $markerPath"
    }
    try {
      $ownedByStandaloneInstaller =
        (Get-Content -LiteralPath $markerPath -Raw -ErrorAction Stop).Trim() -eq $installMarkerVersion
    } catch {
      $ownedByStandaloneInstaller = $false
    }
  }
  if ((Test-Path -LiteralPath $existingCli) -and
      -not $ownedByStandaloneInstaller -and -not $allowReplaceExisting) {
    throw "Existing $existingCli is managed by another installation; refusing to replace it. Remove it, choose a different CCCC_INSTALL_DIR, or set CCCC_ALLOW_REPLACE_EXISTING=1 to replace it deliberately"
  }

  foreach ($binary in $binaries) {
    if (Test-Path -LiteralPath (Join-Path $InstallDir $binary)) {
      $originals += $binary
    }
  }
  New-Item -ItemType Directory -Path $backupDir | Out-Null
  $transactionStarted = $true
  $oldCli = Join-Path $InstallDir "cccc.exe"
  if (Test-Path -LiteralPath $oldCli -PathType Leaf) {
    & $oldCli daemon status *> $null
    $daemonWasRunning = $LASTEXITCODE -eq 0
    if ($daemonWasRunning) {
      & $oldCli daemon stop *> $null
      if ($LASTEXITCODE -ne 0) { throw "Could not stop the running CCCC daemon" }
      for ($attempt = 0; $attempt -lt 40; $attempt++) {
        & $oldCli daemon status *> $null
        if ($LASTEXITCODE -ne 0) { break }
        Start-Sleep -Milliseconds 250
      }
      if ($attempt -eq 40) { throw "The running CCCC daemon did not stop in time" }
    }
  }

  foreach ($binary in $originals) {
    Move-CcccItemWithRetry (Join-Path $InstallDir $binary) (Join-Path $backupDir $binary)
  }
  foreach ($binary in $binaries) {
    $stage = Join-Path $InstallDir (".$binary.cccc-install-" + $PID)
    Move-CcccItemWithRetry $stage (Join-Path $InstallDir $binary)
  }

  $installedVersion = (& (Join-Path $InstallDir "cccc.exe") --version | Out-String).Trim()
  if ($LASTEXITCODE -ne 0 -or $installedVersion -ne "cccc $Version") {
    throw "Installed version mismatch: expected cccc $Version, got $installedVersion"
  }

  if (Test-Path -LiteralPath $markerPath -PathType Leaf) {
    Move-Item -LiteralPath $markerPath -Destination $markerBackup
    $markerOriginal = $true
  }
  $markerTouched = $true
  Set-Content -LiteralPath $markerStage -Value $installMarkerVersion -Encoding Ascii
  Move-Item -LiteralPath $markerStage -Destination $markerPath

  $pathEntries = @($env:Path.Split(';', [StringSplitOptions]::RemoveEmptyEntries))
  $pathReady = $pathEntries.Where({ Test-SameDirectoryPath $_ $InstallDir }).Count -gt 0
  if (-not $NoModifyPath) {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $userPathBeforeInstall = $userPath
    $updatedUserPath = Add-DirectoryToPathFront $userPath $InstallDir
    [Environment]::SetEnvironmentVariable("Path", $updatedUserPath, "User")
    $pathModified = $true
    $env:Path = Add-DirectoryToPathFront $env:Path $InstallDir
    Write-Host "Added $InstallDir to the front of User PATH and the current PowerShell PATH."
  } elseif (-not $pathReady) {
    Write-Warning "Move $InstallDir to the front of PATH, then open a new terminal."
  }

  $resolvedCommands = @(Get-CcccCommandPaths $env:Path)
  $resolvedCommand = if ($resolvedCommands.Count -gt 0) { $resolvedCommands[0] } else { $null }
  if (-not $NoModifyPath -and -not (Test-SameCommandPath $resolvedCommand $installedCommand)) {
    throw "PATH verification resolved cccc to $resolvedCommand instead of $installedCommand"
  }
  if ($NoModifyPath -and -not (Test-SameCommandPath $resolvedCommand $installedCommand)) {
    Write-Warning "This shell still resolves cccc to $resolvedCommand. Run $installedCommand directly or move $InstallDir to the front of PATH."
  }
  $otherCommands = @(
    $resolvedCommands |
      Where-Object { -not (Test-SameCommandPath $_ $installedCommand) } |
      Select-Object -Unique
  )
  if ($otherCommands.Count -gt 0) {
    Write-Host "Other CCCC commands were left unchanged:"
    foreach ($commandPath in $otherCommands) {
      Write-Host "  - $commandPath"
    }
  }

  if (-not $NoModifyPath) {
    $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $persistedUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $persistedPath = Join-PersistedWindowsPath $machinePath $persistedUserPath
    $persistedCommands = @(Get-CcccCommandPaths $persistedPath)
    $persistedCommand = if ($persistedCommands.Count -gt 0) { $persistedCommands[0] } else { $null }
    if (-not (Test-SameCommandPath $persistedCommand $installedCommand)) {
      throw "Persistent PATH verification resolved cccc to $persistedCommand instead of $installedCommand"
    }
    Write-Host "Verified persistent Machine + User PATH resolves cccc to $installedCommand."
  }

  if ($daemonWasRunning) {
    & (Join-Path $InstallDir "cccc.exe") daemon start *> $null
    if ($LASTEXITCODE -ne 0) { throw "The updated CCCC daemon could not restart" }
  }
  $transactionCommitted = $true
  Remove-Item -LiteralPath $backupDir -Recurse -Force

  Write-Host "Installed CCCC v$Version in $InstallDir"
  Write-Host "Verify installed command directly: `"$installedCommand`" doctor"
  Write-Host "Verify after opening a new terminal: cccc doctor"
} finally {
  if ($transactionStarted -and -not $transactionCommitted) {
    if ($pathModified) {
      try {
        [Environment]::SetEnvironmentVariable("Path", $userPathBeforeInstall, "User")
        $env:Path = $processPathBeforeInstall
      } catch {
        Write-Error "Rollback failed to restore PATH: $_" -ErrorAction Continue
      }
    }
    foreach ($binary in $binaries) {
      $destination = Join-Path $InstallDir $binary
      $backup = Join-Path $backupDir $binary
      if ($originals -contains $binary) {
        if (Test-Path -LiteralPath $backup -PathType Leaf) {
          try {
            Remove-Item -LiteralPath $destination -Force -ErrorAction SilentlyContinue
            Move-Item -LiteralPath $backup -Destination $destination -Force
          } catch {
            $rollbackRestoreFailed = $true
            Write-Error "Rollback failed to restore $destination`: $_" -ErrorAction Continue
          }
        }
      } else {
        try {
          if (Test-Path -LiteralPath $destination) {
            Remove-Item -LiteralPath $destination -Force -ErrorAction Stop
          }
        } catch {
          $rollbackRestoreFailed = $true
          Write-Error "Rollback failed to remove $destination`: $_" -ErrorAction Continue
        }
      }
    }
    if ($markerTouched -and -not $rollbackRestoreFailed) {
      Remove-Item -LiteralPath $markerPath -Force -ErrorAction SilentlyContinue
      if ($markerOriginal -and (Test-Path -LiteralPath $markerBackup -PathType Leaf)) {
        try {
          Move-Item -LiteralPath $markerBackup -Destination $markerPath -Force
        } catch {
          $rollbackRestoreFailed = $true
          Write-Error "Rollback failed to restore $markerPath`: $_" -ErrorAction Continue
        }
      }
    }
    if ($daemonWasRunning -and (Test-Path -LiteralPath (Join-Path $InstallDir "cccc.exe"))) {
      try {
        & (Join-Path $InstallDir "cccc.exe") daemon start *> $null
        if ($LASTEXITCODE -ne 0) {
          Write-Error "Rollback restored the previous binary but failed to restart its daemon" -ErrorAction Continue
        }
      } catch {
        Write-Error "Rollback restored the previous binary but failed to restart its daemon: $_" -ErrorAction Continue
      }
    }
  }
  foreach ($stage in $staged) {
    Remove-Item -LiteralPath $stage -Force -ErrorAction SilentlyContinue
  }
  Remove-Item -LiteralPath $markerStage -Force -ErrorAction SilentlyContinue
  if ($rollbackRestoreFailed) {
    Write-Error "Previous binary backup retained at $backupDir" -ErrorAction Continue
  } else {
    Remove-Item -LiteralPath $backupDir -Recurse -Force -ErrorAction SilentlyContinue
  }
  Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($null -ne $lockStream) {
    $lockStream.Dispose()
    Remove-Item -LiteralPath $lockPath -Force -ErrorAction SilentlyContinue
  }
}
