function Test-InstallerConcurrentSnapshot(
  [string]$RootDir,
  [string]$TempRoot,
  [string]$InstallDir,
  [string]$RealVersion,
  [string]$RealBinary
) {
  $failingVersion = "9.9.7"
  New-FixtureRelease $failingVersion $true
  $signal = Join-Path $TempRoot "stale-lock-reached"
  $release = Join-Path $TempRoot "stale-lock-release"
  $source = Get-Content -LiteralPath (Join-Path $RootDir "scripts\install.ps1") -Raw
  $lockLine = '    $lockStream = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)'
  $pause = @"
    [IO.File]::WriteAllText(`$env:CCCC_TEST_LOCK_SIGNAL, "")
    while (-not (Test-Path -LiteralPath `$env:CCCC_TEST_LOCK_RELEASE)) { Start-Sleep -Milliseconds 10 }
$lockLine
"@
  if (-not $source.Contains($lockLine)) { throw "could not inject the installer lock pause" }
  $pausedInstaller = Join-Path $TempRoot "install-paused-before-lock.ps1"
  $source.Replace($lockLine, $pause) | Set-Content -LiteralPath $pausedInstaller

  $env:CCCC_TEST_LOCK_SIGNAL = $signal
  $env:CCCC_TEST_LOCK_RELEASE = $release
  $childOut = Join-Path $TempRoot "stale-install.out"
  $childErr = Join-Path $TempRoot "stale-install.err"
  $hostExecutable = (Get-Process -Id $PID).Path
  $child = Start-Process -FilePath $hostExecutable -PassThru -NoNewWindow -ArgumentList @(
    "-NoProfile", "-File", $pausedInstaller,
    "-Version", $failingVersion, "-InstallDir", $InstallDir, "-NoModifyPath"
  ) -RedirectStandardOutput $childOut -RedirectStandardError $childErr
  try {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (-not $child.HasExited -and -not (Test-Path -LiteralPath $signal) -and [DateTime]::UtcNow -lt $deadline) {
      Start-Sleep -Milliseconds 10
    }
    if (-not (Test-Path -LiteralPath $signal)) { throw "second installer did not reach its transaction lock" }
    & (Join-Path $RootDir "scripts\install.ps1") -Version $RealVersion -InstallDir $InstallDir -NoModifyPath
    New-Item -ItemType File -Path $release | Out-Null
    $child.WaitForExit()
    if ($child.ExitCode -eq 0) { throw "stale installer accepted a mismatched binary version" }
    $installed = Join-Path $InstallDir "cccc.exe"
    if (-not (Test-Path -LiteralPath $installed -PathType Leaf) -or
        (Get-FileHash $installed).Hash -ne (Get-FileHash $RealBinary).Hash) {
      throw "stale rollback removed the concurrently installed binary"
    }
  } finally {
    if (-not $child.HasExited) { $child.Kill() }
    Remove-Item Env:CCCC_TEST_LOCK_SIGNAL -ErrorAction SilentlyContinue
    Remove-Item Env:CCCC_TEST_LOCK_RELEASE -ErrorAction SilentlyContinue
  }
}
