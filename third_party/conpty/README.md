# Microsoft ConPTY (redistributable)

`shellman-terminal` makes its pseudo-consoles through this `conpty.dll`, which starts this
`OpenConsole.exe` as the console host, instead of Windows' inbox `conhost.exe` (much faster at
passing output through; see `docs/PLAN.md`, Terminal). Without these two files beside the exe the
terminal falls back to Windows' own ConPTY.

- Package: NuGet `Microsoft.Windows.Console.ConPTY` **1.25.260930003** (stable, published
  2026-10-02), authors Microsoft, project https://github.com/microsoft/terminal
- Downloaded from nuget.org only:
  https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/1.25.260930003/microsoft.windows.console.conpty.1.25.260930003.nupkg
  - `.nupkg` SHA-512 `2EEA8C93DAF9A13CDE4677B7E483A551D490EC3923A2B3886DC9CE0C667CD4CBE65294EBFF5541433DDF01D08D0B091FB91D0C96EABE1FBAD2DC77ACFC0D5F3B`
    (equal to the nuget.org catalog's `packageHash`)
- Licence: MIT (the package's licence expression; `LICENSE` here is the microsoft/terminal
  repository's, `NOTICE.md` its third-party notices, both from its `main` branch).
- Signatures: both files Authenticode-valid, signer `CN=Microsoft Corporation, O=Microsoft
  Corporation, L=Redmond, S=Washington, C=US`, issuer `Microsoft Code Signing PCA 2024`.

| File here | From the package | Version | Size | SHA-256 |
|---|---|---|---|---|
| `x64/conpty.dll` | `runtimes/win-x64/native/conpty.dll` | 1.25.2609.30003 | 119 608 | `FEEEF341D891643C62D30B6B07800BC70F0BC148F44CB8C3BEE84AA557AE805A` |
| `x64/OpenConsole.exe` | `build/native/runtimes/x64/OpenConsole.exe` | 1.25.2609.30003 | 1 089 376 | `3D66B23D0A71BB8EED2B77EDC8B9DF9BF54CE6C8FB74C863A30E760997F80586` |

## Where they go

Both files go **in the same folder as `shellman-terminal.exe`** (the install folder,
`%LOCALAPPDATA%\Programs\shellman`). `apps/terminal/build.rs` copies them next to the exe in
`target\<profile>\` on every build. `conpty.dll` looks for `OpenConsole.exe` beside itself (then in
an `x64\` subfolder); the terminal only uses the dll when both are there, and loads it by full path
(never from PATH). An update must treat them like the exes: a running terminal keeps
`conpty.dll` loaded and each tab runs an `OpenConsole.exe`, so they are renamed aside, not
overwritten, while terminals are open.

ARM64 and x86 builds of both are in the same package (not copied here; shellman builds x64).

To update: download the new version from nuget.org, check the hash against the catalog, check both
signatures (`Get-AuthenticodeSignature`), copy the two x64 files here and update this table.
