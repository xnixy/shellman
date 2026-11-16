<#
.SYNOPSIS
  Removes exactly what install.ps1 added to ONE Firefox profile and restores the backup.
.DESCRIPTION
  Same targeting rule as install.ps1: -Profile <folder> or -DefaultProfile, never implicit.
  Deletes the five theme files, removes the marked block from user.js, puts back any chrome\ files
  or user.js that the install had overwritten/created from the backup, and removes the backup folder.
  Prefs Firefox already copied into prefs.js on exit are not edited: see README ("Uninstalling").
  If install.ps1 -Scripts was used, also deletes chrome\shellman-scripts\ (with shellman_glass.dll, if -GlassDll
  installed it; Firefox must be closed, it keeps the DLL loaded) and the two autoconfig files from the
  Firefox folder it recorded (only if they are still exactly the files it installed), and puts back any
  autoconfig files they replaced. That folder usually needs an elevated PowerShell; the script checks first.
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

# 0. scripts: checked first, so a missing admin right leaves everything installed and consistent
if ($state.PSObject.Properties['firefoxDir'] -and $state.firefoxDir) {
    $ffDir = $state.firefoxDir
    if (Test-Path -LiteralPath $ffDir) {
        Assert-Writable $ffDir
        foreach ($a in $state.autoconfig) {
            $dst = Join-Path $ffDir $a.path
            if (Test-Path -LiteralPath $dst) {
                if ((Get-Sha256 $dst) -eq $a.sha256) { Remove-Item -LiteralPath $dst -Force }
                else { Write-Warning "$dst changed since install; left in place." }
            }
            $old = Join-Path $backupDir ('autoconfig\' + $a.path)
            if ($a.backedUp -and (Test-Path -LiteralPath $old) -and -not (Test-Path -LiteralPath $dst)) { Copy-Item -LiteralPath $old -Destination $dst -Force }
        }
    } else {
        Write-Warning "Firefox folder $ffDir no longer exists; nothing to remove there."
    }
    $sd = Join-Path $chromeDir $script:ScriptsDir
    foreach ($f in $state.scripts) { $p = Join-Path $sd $f; if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Force } }
    if ((Test-Path -LiteralPath $sd) -and -not (Get-ChildItem -LiteralPath $sd -Force)) { Remove-Item -LiteralPath $sd -Force }
}

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
    # The line break install added after a user.js that did not end in one.
    if ($state.PSObject.Properties['userJsBreak'] -and $state.userJsBreak -and $rest -and $rest.EndsWith("`r`n")) { $rest = $rest.Substring(0, $rest.Length - 2) }
    if ((-not $state.hadUserJs) -and [string]::IsNullOrWhiteSpace($rest)) { Remove-Item -LiteralPath $userJs -Force }
    else { [System.IO.File]::WriteAllText($userJs, $rest, (New-Object System.Text.UTF8Encoding($false))) }
}
if ($state.hadUserJs -and -not (Test-Path -LiteralPath $userJs)) {
    Copy-Item -LiteralPath (Join-Path $backupDir 'user.js') -Destination $userJs -Force
}

# 3. the backup folder goes last
Remove-Item -LiteralPath $backupRoot -Recurse -Force
Write-Host "Uninstalled from $target. Restart Firefox."
