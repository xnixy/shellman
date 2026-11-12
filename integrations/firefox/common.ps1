# Shared helpers for install.ps1 / uninstall.ps1. Dot-sourced, not run directly.

$script:BeginMark = '// >>> shellman-islands begin (managed by integrations/firefox/install.ps1, do not edit) >>>'
$script:EndMark   = '// <<< shellman-islands end <<<'
$script:BackupRoot = 'shellman-islands-backup'
$script:ChromeFiles = @('userChrome.css', 'userContent.css', 'shellman-look.css', 'shellman-islands.css', 'shellman-popups.css')

# Reads profiles.ini (read only) and returns the default-release profile folder.
function Get-DefaultProfilePath {
    $root = Join-Path $env:APPDATA 'Mozilla\Firefox'
    $ini = Join-Path $root 'profiles.ini'
    if (-not (Test-Path -LiteralPath $ini)) { throw "profiles.ini not found at $ini" }
    $sections = @{}; $cur = $null
    foreach ($line in Get-Content -LiteralPath $ini) {
        if ($line -match '^\s*\[(.+)\]\s*$') { $cur = $Matches[1]; $sections[$cur] = @{}; continue }
        if ($cur -and $line -match '^\s*([^=;#]+?)\s*=\s*(.*?)\s*$') { $sections[$cur][$Matches[1]] = $Matches[2] }
    }
    $rel = $null
    # Firefox 67+: the [Install...] section names the profile this installation really uses.
    foreach ($k in $sections.Keys) { if ($k -like 'Install*' -and $sections[$k].ContainsKey('Default')) { $rel = @{ Path = $sections[$k]['Default']; IsRelative = '1' }; break } }
    if (-not $rel) {
        foreach ($k in $sections.Keys) { if ($k -like 'Profile*' -and $sections[$k]['Default'] -eq '1') { $rel = @{ Path = $sections[$k]['Path']; IsRelative = $sections[$k]['IsRelative'] }; break } }
    }
    if (-not $rel) {
        foreach ($k in $sections.Keys) { if ($k -like 'Profile*' -and $sections[$k]['Name'] -eq 'default-release') { $rel = @{ Path = $sections[$k]['Path']; IsRelative = $sections[$k]['IsRelative'] }; break } }
    }
    if (-not $rel) { throw 'No default profile found in profiles.ini; pass -Profile <path> instead.' }
    $p = $rel.Path -replace '/', '\'
    if ($rel.IsRelative -ne '0') { $p = Join-Path $root $p }
    return $p
}

# Resolves and validates the target profile. Refuses to guess.
function Resolve-TargetProfile([string]$Profile, [bool]$UseDefault) {
    if ($Profile -and $UseDefault) { throw 'Pass either -Profile or -DefaultProfile, not both.' }
    if (-not $Profile -and -not $UseDefault) {
        throw "Refusing to run without an explicit target. Pass -Profile <path-to-profile-folder>, or -DefaultProfile to use the default-release profile from profiles.ini."
    }
    if ($UseDefault) { $Profile = Get-DefaultProfilePath }
    if (-not (Test-Path -LiteralPath $Profile -PathType Container)) { throw "Profile folder does not exist: $Profile" }
    return (Resolve-Path -LiteralPath $Profile).Path.TrimEnd('\')
}

# True when a running Firefox holds the profile (the profile's lock file is held open).
function Test-ProfileInUse([string]$Profile) {
    foreach ($name in 'parent.lock', 'lock') {
        $f = Join-Path $Profile $name
        if (Test-Path -LiteralPath $f) {
            try { $s = [System.IO.File]::Open($f, 'Open', 'ReadWrite', 'None'); $s.Close() } catch { return $true }
        }
    }
    return $false
}

function Remove-ManagedBlock([string]$Text) {
    $pattern = '(?s)' + [regex]::Escape($script:BeginMark) + '.*?' + [regex]::Escape($script:EndMark) + '[ \t]*\r?\n?'
    return [regex]::Replace($Text, $pattern, '')
}
