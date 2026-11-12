<#
.SYNOPSIS
  Installs the shellman Islands theme into ONE Firefox profile.
.DESCRIPTION
  Refuses to run unless you name the profile: -Profile <folder> or -DefaultProfile.
  Backs up an existing chrome\ folder and user.js into <profile>\shellman-islands-backup\<timestamp>\,
  copies the theme into <profile>\chrome\, and appends the prefs from user.js between marker comments.
  Running it again updates the files without making a second backup. Nothing else in the profile is touched.
.EXAMPLE
  .\install.ps1 -DefaultProfile
  .\install.ps1 -Profile C:\Temp\test-profile
#>
[CmdletBinding()]
param(
    [string]$Profile,
    [switch]$DefaultProfile,
    # Install even if the profile looks in use. Firefox must be restarted either way.
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

$target = Resolve-TargetProfile $Profile ([bool]$DefaultProfile)
if ((Test-ProfileInUse $target) -and -not $Force) {
    throw "Firefox is running with this profile ($target). Close it first (or pass -Force; Firefox would overwrite user.js-derived prefs on exit)."
}

$srcChrome = Join-Path $PSScriptRoot 'chrome'
$srcPrefs = Join-Path $PSScriptRoot 'user.js'
foreach ($f in $script:ChromeFiles) { if (-not (Test-Path -LiteralPath (Join-Path $srcChrome $f))) { throw "Missing source file chrome\$f" } }
if (-not (Test-Path -LiteralPath $srcPrefs)) { throw 'Missing source file user.js' }

$chromeDir = Join-Path $target 'chrome'
$userJs = Join-Path $target 'user.js'
$backupRoot = Join-Path $target $script:BackupRoot
$stateFile = Join-Path $backupRoot 'state.json'

if (Test-Path -LiteralPath $stateFile) {
    Write-Host 'Already installed here: updating files, keeping the original backup.'
    $state = Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
} else {
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $backupDir = Join-Path $backupRoot $stamp
    New-Item -ItemType Directory -Force -Path $backupDir | Out-Null
    $hadChrome = Test-Path -LiteralPath $chromeDir
    $hadUserJs = Test-Path -LiteralPath $userJs
    if ($hadChrome) { Copy-Item -LiteralPath $chromeDir -Destination (Join-Path $backupDir 'chrome') -Recurse -Force }
    if ($hadUserJs) { Copy-Item -LiteralPath $userJs -Destination (Join-Path $backupDir 'user.js') -Force }
    $state = [pscustomobject]@{
        backup     = $stamp
        hadChrome  = $hadChrome
        hadUserJs  = $hadUserJs
        files      = $script:ChromeFiles
        installed  = (Get-Date).ToString('o')
    }
    $state | ConvertTo-Json | Set-Content -LiteralPath $stateFile -Encoding UTF8
    Write-Host "Backup: $backupDir"
}

# 1. chrome\ files (only ours are written; other files in chrome\ are left alone)
New-Item -ItemType Directory -Force -Path $chromeDir | Out-Null
foreach ($f in $script:ChromeFiles) { Copy-Item -LiteralPath (Join-Path $srcChrome $f) -Destination (Join-Path $chromeDir $f) -Force }

# 2. prefs between markers, appended to any existing user.js
$existing = ''
if (Test-Path -LiteralPath $userJs) { $existing = Remove-ManagedBlock (Get-Content -LiteralPath $userJs -Raw) }
if ($existing -and -not $existing.EndsWith("`n")) { $existing += "`r`n" }
$block = $script:BeginMark + "`r`n" + ((Get-Content -LiteralPath $srcPrefs -Raw).TrimEnd()) + "`r`n" + $script:EndMark + "`r`n"
[System.IO.File]::WriteAllText($userJs, $existing + $block, (New-Object System.Text.UTF8Encoding($false)))

Write-Host "Installed into $target"
Write-Host 'Restart Firefox for the theme to apply (pass -purgecaches once if it looks unchanged).'
