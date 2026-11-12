<#
.SYNOPSIS
  Removes exactly what install.ps1 added to ONE Firefox profile and restores the backup.
.DESCRIPTION
  Same targeting rule as install.ps1: -Profile <folder> or -DefaultProfile, never implicit.
  Deletes the five theme files, removes the marked block from user.js, puts back any chrome\ files
  or user.js that the install had overwritten/created from the backup, and removes the backup folder.
  Prefs Firefox already copied into prefs.js on exit are not edited: see README ("Uninstalling").
#>
[CmdletBinding()]
param(
    [string]$Profile,
    [switch]$DefaultProfile,
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

$target = Resolve-TargetProfile $Profile ([bool]$DefaultProfile)
if ((Test-ProfileInUse $target) -and -not $Force) { throw "Firefox is running with this profile ($target). Close it first (or pass -Force)." }

$chromeDir = Join-Path $target 'chrome'
$userJs = Join-Path $target 'user.js'
$backupRoot = Join-Path $target $script:BackupRoot
$stateFile = Join-Path $backupRoot 'state.json'
if (-not (Test-Path -LiteralPath $stateFile)) { Write-Host 'Nothing to uninstall (no shellman Islands state in this profile).'; return }
$state = Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
$backupDir = Join-Path $backupRoot $state.backup

# 1. our files out, then whatever chrome\ files we overwrote back in
foreach ($f in $state.files) {
    $p = Join-Path $chromeDir $f
    if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Force }
    $old = Join-Path (Join-Path $backupDir 'chrome') $f
    if (Test-Path -LiteralPath $old) { New-Item -ItemType Directory -Force -Path $chromeDir | Out-Null; Copy-Item -LiteralPath $old -Destination $p -Force }
}
if ((-not $state.hadChrome) -and (Test-Path -LiteralPath $chromeDir) -and -not (Get-ChildItem -LiteralPath $chromeDir -Force)) { Remove-Item -LiteralPath $chromeDir -Force }

# 2. user.js: drop our block (keeps anything the user added after install); remove the file if we created it
if (Test-Path -LiteralPath $userJs) {
    $rest = Remove-ManagedBlock (Get-Content -LiteralPath $userJs -Raw)
    if ((-not $state.hadUserJs) -and [string]::IsNullOrWhiteSpace($rest)) { Remove-Item -LiteralPath $userJs -Force }
    else { [System.IO.File]::WriteAllText($userJs, $rest, (New-Object System.Text.UTF8Encoding($false))) }
}
if ($state.hadUserJs -and -not (Test-Path -LiteralPath $userJs)) {
    Copy-Item -LiteralPath (Join-Path $backupDir 'user.js') -Destination $userJs -Force
}

# 3. the backup folder goes last
Remove-Item -LiteralPath $backupRoot -Recurse -Force
Write-Host "Uninstalled from $target. Restart Firefox."
