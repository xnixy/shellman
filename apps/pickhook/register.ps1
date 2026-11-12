# Makes this user's apps get the Files picker for their Open and Save dialogs, by registering
# shellman_pickhook.dll as the per-user in-process server for CLSID_FileOpenDialog and
# CLSID_FileSaveDialog (HKCU only; nothing machine-wide). See README.md before running this.
# Undo with unregister.ps1. Takes effect for programs started afterwards.
param(
    [string]$DllPath = (Join-Path $env:LOCALAPPDATA 'Programs\shellman\shellman_pickhook.dll')
)
$ErrorActionPreference = 'Stop'

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this from a normal (not elevated) PowerShell: the registration is per user.'
}
$DllPath = [IO.Path]::GetFullPath($DllPath)
if (-not (Test-Path -LiteralPath $DllPath -PathType Leaf)) { throw "No DLL at $DllPath" }
$picker = Join-Path (Split-Path $DllPath) 'shellman-files.exe'
if (-not (Test-Path -LiteralPath $picker -PathType Leaf)) { throw "The picker must sit next to the DLL: no $picker" }

$classes = @{
    '{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}' = 'File Open Dialog (shellman picker hook)'
    '{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}' = 'File Save Dialog (shellman picker hook)'
}
foreach ($clsid in $classes.Keys) {
    $key = "HKCU:\Software\Classes\CLSID\$clsid"
    if (Test-Path $key) { throw "$key already exists; not touching it. Run unregister.ps1 first if it is ours." }
}
foreach ($clsid in $classes.Keys) {
    $key = "HKCU:\Software\Classes\CLSID\$clsid"
    New-Item -Path $key -Force | Out-Null
    Set-ItemProperty -Path $key -Name '(default)' -Value $classes[$clsid]
    New-Item -Path "$key\InprocServer32" -Force | Out-Null
    Set-ItemProperty -Path "$key\InprocServer32" -Name '(default)' -Value $DllPath
    Set-ItemProperty -Path "$key\InprocServer32" -Name 'ThreadingModel' -Value 'Apartment'
    "registered $clsid -> $DllPath"
}
