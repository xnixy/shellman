# Undoes register.ps1: removes the per-user registrations of CLSID_FileOpenDialog and
# CLSID_FileSaveDialog, but only where they point at shellman_pickhook.dll. Windows' own
# registrations (HKLM) are never touched; apps get the real dialogs again from their next start.
$ErrorActionPreference = 'Stop'
foreach ($clsid in '{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}', '{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}') {
    $key = "HKCU:\Software\Classes\CLSID\$clsid"
    if (-not (Test-Path $key)) { "not registered: $clsid"; continue }
    $server = (Get-ItemProperty -Path "$key\InprocServer32" -ErrorAction SilentlyContinue).'(default)'
    if ($server -notlike '*shellman_pickhook.dll') { "left alone: $key points at '$server', not the hook"; continue }
    Remove-Item -Path $key -Recurse
    "removed $key"
}
