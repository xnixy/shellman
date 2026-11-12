<#
.SYNOPSIS
  Installs the shellman Islands theme into ONE Firefox profile.
.DESCRIPTION
  Refuses to run unless you name the profile: -Profile <folder> or -DefaultProfile.
  Backs up an existing chrome\ folder and user.js into <profile>\shellman-islands-backup\<timestamp>\,
  copies the theme into <profile>\chrome\, and appends the prefs from user.js between marker comments.
  Running it again updates the files without making a second backup. Nothing else in the profile is touched.

  -Scripts (opt-in) also installs the live-look script: chrome\shellman-scripts\ in the profile, and the two
  autoconfig files (config.js, defaults\pref\config-prefs.js) into the Firefox install folder named by
  -FirefoxDir. That folder is usually under Program Files and then needs an elevated PowerShell; the script
  checks and stops with a message instead of asking for elevation. See README ("Scripts").

  -GlassDll <path> (with -Scripts, opt-in) also copies shellman_glass.dll (cargo build --release -p
  shellman-firefox-glass) into chrome\shellman-scripts\: real see-through gaps with shellman's own glass under
  each island. See README ("Real gaps").
.EXAMPLE
  .\install.ps1 -DefaultProfile
  .\install.ps1 -Profile C:\Temp\test-profile
  .\install.ps1 -DefaultProfile -Scripts -FirefoxDir "C:\Program Files\Mozilla Firefox"
  .\install.ps1 -DefaultProfile -Scripts -FirefoxDir "C:\Program Files\Mozilla Firefox" -GlassDll ..\..\target\release\shellman_glass.dll
#>
[CmdletBinding()]
param(
    [string]$Profile,
    [switch]$DefaultProfile,
    # Also install the live-look script and its autoconfig loader (needs -FirefoxDir).
    [switch]$Scripts,
    [string]$FirefoxDir,
    # Also install shellman_glass.dll (real gaps; needs -Scripts).
    [string]$GlassDll,
    # Install even if the profile looks in use. Firefox must be restarted either way.
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

$target = Resolve-TargetProfile $Profile ([bool]$DefaultProfile)
$ffDir = $null
if ($Scripts) {
    # Checked before anything is written, so a missing admin right leaves the profile untouched.
    $ffDir = Resolve-FirefoxDir $FirefoxDir
    Assert-Writable $ffDir
    $known = @()
    $sf = Join-Path (Join-Path $target $script:BackupRoot) 'state.json'
    if (Test-Path -LiteralPath $sf) { $st = Get-Content -LiteralPath $sf -Raw | ConvertFrom-Json; if ($st.PSObject.Properties['autoconfig']) { $known = @($st.autoconfig | ForEach-Object { $_.path }) } }
    foreach ($rel in $script:AutoconfigFiles.Keys) {
        $dst = Join-Path $ffDir $rel
        if (($known -notcontains $rel) -and (Test-Path -LiteralPath $dst) -and -not ((Get-Content -LiteralPath $dst -Raw) -match 'shellman Islands') -and -not $Force) {
            throw "$dst already exists and is not shellman's (another autoconfig setup?). Pass -Force to replace it; it is backed up and put back on uninstall."
        }
    }
} elseif ($FirefoxDir) {
    throw '-FirefoxDir is only used together with -Scripts.'
}
if ($GlassDll) {
    if (-not $Scripts) { throw '-GlassDll is only used together with -Scripts.' }
    if (-not (Test-Path -LiteralPath $GlassDll -PathType Leaf)) { throw "No file at $GlassDll (build it with: cargo build --release -p shellman-firefox-glass)" }
    if ((Split-Path $GlassDll -Leaf) -ne $script:GlassDllName) { throw "-GlassDll must point to $($script:GlassDllName)" }
    $GlassDll = (Resolve-Path -LiteralPath $GlassDll).Path
}
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

# 3. scripts (opt-in)
if ($Scripts) {
    $backupDir = Join-Path $backupRoot $state.backup
    $srcScripts = Join-Path $srcChrome $script:ScriptsDir
    $srcAuto = Join-Path $PSScriptRoot 'autoconfig'
    foreach ($f in $script:ScriptFiles) { if (-not (Test-Path -LiteralPath (Join-Path $srcScripts $f))) { throw "Missing source file chrome\$($script:ScriptsDir)\$f" } }
    foreach ($f in $script:AutoconfigFiles.Values) { if (-not (Test-Path -LiteralPath (Join-Path $srcAuto $f))) { throw "Missing source file autoconfig\$f" } }
    if ($state.PSObject.Properties['firefoxDir'] -and $state.firefoxDir -and ($state.firefoxDir -ne $ffDir)) {
        throw "Scripts are already installed for $($state.firefoxDir). Uninstall first to switch Firefox folders."
    }
    $previous = @{}
    if ($state.PSObject.Properties['autoconfig']) { foreach ($a in $state.autoconfig) { $previous[$a.path] = $a } }

    # autoconfig files next to firefox.exe; a foreign autoconfig is never overwritten silently
    $records = @()
    foreach ($rel in $script:AutoconfigFiles.Keys) {
        $dst = Join-Path $ffDir $rel
        $src = Join-Path $srcAuto $script:AutoconfigFiles[$rel]
        $backedUp = $false
        if ($previous.ContainsKey($rel)) { $backedUp = [bool]$previous[$rel].backedUp }
        elseif (Test-Path -LiteralPath $dst) {

            $bdir = Join-Path $backupDir ('autoconfig\' + (Split-Path $rel -Parent))
            New-Item -ItemType Directory -Force -Path $bdir | Out-Null
            Copy-Item -LiteralPath $dst -Destination (Join-Path $bdir (Split-Path $rel -Leaf)) -Force
            $backedUp = $true
        }
        Copy-Item -LiteralPath $src -Destination $dst -Force
        $records += [pscustomobject]@{ path = $rel; sha256 = (Get-Sha256 $dst); backedUp = $backedUp }
    }

    # the scripts themselves, in their own folder in the profile (the only folder the loader reads)
    $dstScripts = Join-Path $chromeDir $script:ScriptsDir
    New-Item -ItemType Directory -Force -Path $dstScripts | Out-Null
    foreach ($f in $script:ScriptFiles) { Copy-Item -LiteralPath (Join-Path $srcScripts $f) -Destination (Join-Path $dstScripts $f) -Force }
    $installed = @($script:ScriptFiles)
    if ($GlassDll) {
        Copy-Item -LiteralPath $GlassDll -Destination (Join-Path $dstScripts $script:GlassDllName) -Force
        $installed += $script:GlassDllName
    } elseif ($state.PSObject.Properties['scripts'] -and (@($state.scripts) -contains $script:GlassDllName)) {
        $installed += $script:GlassDllName # installed earlier: kept, and still removed on uninstall
    }

    $state | Add-Member -NotePropertyName firefoxDir -NotePropertyValue $ffDir -Force
    $state | Add-Member -NotePropertyName autoconfig -NotePropertyValue $records -Force
    $state | Add-Member -NotePropertyName scripts -NotePropertyValue $installed -Force
    $state | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $stateFile -Encoding UTF8
    Write-Host "Scripts: loader in $ffDir, live look in $dstScripts"
}

Write-Host "Installed into $target"
Write-Host 'Restart Firefox for the theme to apply (pass -purgecaches once if it looks unchanged).'
