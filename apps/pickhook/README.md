# shellman pick hook

`shellman_pickhook.dll` lets other apps' **Open** and **Save** dialogs be the Files picker
(`shellman-files --pick`). It is an in-process COM server for Windows' two dialog classes:

| Class | CLSID |
|---|---|
| `CLSID_FileOpenDialog` | `{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}` |
| `CLSID_FileSaveDialog` | `{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}` |

**Nothing registers it.** It is built and tested, but no app gets it until someone runs
`register.ps1`. Read the risks below first.

## How it works

Each dialog object the hook makes owns a *real* Windows dialog, created straight from
`C:\Windows\System32\comdlg32.dll` through its `DllGetClassObject` (never through the registry,
so it reaches the real dialog even while the hook is registered). Every call the app makes is
passed on to that real dialog as it comes (file types, options, folder, title, custom controls,
event sinks...), and the hook notes what the picker needs. On `Show(owner)`:

* If the app used nothing the picker cannot do, the hook writes a request to `%TEMP%`, starts
  `shellman-files.exe --pick <request>` (from the DLL's own folder), disables the owner window
  as a modal dialog does, and pumps the app's messages until the picker answers. The picker is
  centred over and owned by the app's window. A choice is checked with the app's `OnFileOk`
  sinks before the picker closes; a refusal (`S_FALSE`) keeps the picker open. `Show` then
  returns `S_OK` and `GetResult`/`GetResults`/`GetFileName`/`GetFolder`/`GetFileTypeIndex`
  answer from the choice, or `HRESULT_FROM_WIN32(ERROR_CANCELLED)` on cancel.
* Otherwise, or if the picker cannot be started, or exits without an answer (exit code other
  than 0 = OK or 1 = cancelled), the hook calls `Show` on the real dialog, which already holds
  everything, and the app never knows.

The real dialog is shown instead when the app:

* adds any custom control (`IFileDialogCustomize`: combo boxes, check buttons, the Open
  drop-down, ...), e.g. an encoding choice in a Save dialog;
* sets an item filter (`SetFilter`) or a navigation root (`SetNavigationRoot`);
* asks for properties to edit (`IFileSaveDialog::SetCollectedProperties`);
* combines options the picker has no answer for: `FOS_PICKFOLDERS` with
  `FOS_ALLOWMULTISELECT` or in a save dialog, `FOS_CREATEPROMPT`.

`SHELLMAN_PICKHOOK=off` in an app's environment gives it the real dialogs only.
`SHELLMAN_PICKHOOK_LOG=<file>` logs each decision (picker or real dialog, and why).

Every entry point catches panics and returns an error code; the DLL never unloads
(`DllCanUnloadNow` is `S_FALSE`), and has no dependencies beyond `windows` and the small
`shellman-pick` crate (no egui).

## What registering writes (and unregistering removes)

`register.ps1` (run from a normal, not elevated, PowerShell) writes, for the current user only:

```
HKCU\Software\Classes\CLSID\{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}
    (default)            = "File Open Dialog (shellman picker hook)"
    InprocServer32\
        (default)        = "%LOCALAPPDATA%\Programs\shellman\shellman_pickhook.dll" (expanded)
        ThreadingModel   = "Apartment"
HKCU\Software\Classes\CLSID\{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}
    (default)            = "File Save Dialog (shellman picker hook)"
    InprocServer32\
        (default)        = (the same DLL path)
        ThreadingModel   = "Apartment"
```

It refuses to run elevated, if the DLL or `shellman-files.exe` beside it is missing, or if either
key already exists. `unregister.ps1` deletes exactly those two keys, and only when their
`InprocServer32` points at `shellman_pickhook.dll`. HKLM (Windows' own registration,
`comdlg32.dll`, Apartment) is never touched. Both take effect for programs started afterwards;
running programs keep what they loaded.

## Feasibility on this machine (Windows 11 build 26300)

* **The classes are ordinary.** HKLM has a plain `InprocServer32 = C:\Windows\System32\comdlg32.dll`,
  `ThreadingModel = Apartment` for both CLSIDs (and the same under `WOW6432Node`): no `TreatAs`,
  no `AppID`, no elevation or protection entries. HKCU has no entry for either today.
* **`CoCreateInstance` goes through COM's ordinary class lookup.** Tested without writing the
  registry: the test host (`examples/host.rs`, scenario `actctx`) activates, on its own thread
  only, an activation context whose manifest names the hook DLL as the server for both CLSIDs.
  `CoCreateInstance` then returned objects from `C:\Temp\picktest\actctx\shellman_pickhook.dll`;
  before activating it and after deactivating it, from `C:\Windows\System32\COMDLG32.dll`. So
  the lookup can be redirected; a per-user registration uses the same lookup through the merged
  `HKEY_CLASSES_ROOT` view, where `HKCU\Software\Classes` wins over `HKLM\Software\Classes`.
  The per-user registration itself was **not** tried (no registry writes were allowed).
* **The legacy API is not proven to be covered.** Under the same activation context,
  `GetOpenFileNameW` showed the real dialog, not the hook. This does not settle it: comdlg32
  activates its own activation context around its UI, which hides the thread's, so the
  experiment cannot tell whether a registry override would catch it. Expect old
  `GetOpenFileName`/`GetSaveFileName` apps to be covered only if comdlg32 creates its dialog with
  `CoCreateInstance`; verify after registering.
* **The real dialog works without Explorer.** With explorer.exe not running, the fallback showed
  a working real Open dialog (folder, file types and events as the app set them), which was then
  cancelled.

## Which apps would and would not get the picker

Would (once registered), in their next start:

* 64-bit, medium-integrity desktop apps that create the dialog with `CoCreateInstance`
  (`IFileOpenDialog`/`IFileSaveDialog`) and add no custom controls: most Win32, WinForms/WPF
  (`OpenFileDialog` uses the same classes), Qt and Electron apps on their default dialogs.

Would not:

* **Elevated (administrator) apps**: COM ignores per-user (HKCU) class registrations in
  elevated processes, by design against elevation through user-writable keys. They keep the
  real dialog. (Documented behaviour; not tested, as nothing may trigger UAC here.)
* **32-bit apps**: they read `HKCU\Software\Classes\WOW6432Node\CLSID`, which is not written,
  and could not load an x64 DLL anyway. Covering them needs a 32-bit build of this DLL
  (`--target i686-pc-windows-msvc`) registered there; not built.
* **UWP and other packaged apps using `Windows.Storage.Pickers`**: the picker runs in the
  `PickerHost.exe` broker, not in the app. Whether the broker reads the per-user registration
  is unknown; it would mean loading this DLL into a system broker, which should be checked
  separately before relying on it.
* **Apps that draw their own picker** (Chromium's and Firefox's when configured to, GTK/Java
  file choosers, many games and editors), and apps that call the pre-Vista
  `GetOpenFileName` with hooks or templates (`OFN_ENABLEHOOK`), which comdlg32 serves with the
  old dialog.
* **Apps that add custom controls** (Notepad-style encoding boxes, "Open read-only", "Save
  thumbnail") or use the features listed above: they get the real dialog, by design.
* Apps whose antivirus or EDR policy blocks or flags the registration: replacing a system
  CLSID's server under HKCU is a known persistence technique ("COM hijacking", MITRE ATT&CK
  T1546.015), so security software may warn, quarantine the DLL, or undo the keys.

## Risks and failure modes

* **Every dialog in every 64-bit app loads this DLL.** A crash in it would crash that app. All
  entry points catch panics, and the code only marshals to the picker, but a bug outside Rust's
  panic model (a bad pointer from an app, stack exhaustion) cannot be caught. Keep
  `unregister.ps1` at hand, and `SHELLMAN_PICKHOOK=off` for a single app.
* **Picker missing or broken**: the real dialog is shown (tested: picker exe missing, and a
  picker that exits with code 2).
* **Picker crash mid-use**: the user sees the real dialog next (the run counts as failed).
* **App quits during the picker** (`WM_QUIT`): the picker is stopped and the quit re-posted.
  **App calls `Close(hr)`** from another callback: the picker is stopped and `Show` returns `hr`.
  **App process dies**: the picker notices within a second and cancels itself.
* **Behaviour differences** from the real dialog: `OnFolderChange`, `OnSelectionChange`,
  `OnTypeChange` and `OnOverwrite` are not raised while the picker is up (the picker asks
  "Replace?" itself); `OnFileOk` is raised, and returning `E_NOTIMPL` from it counts as accept.
  Results are always file-system paths. The picker remembers the last folder per program (and
  per `SetClientGuid` id) in `%APPDATA%\shellman\files-pick.json`, not in Windows' own MRU.
* **Focus**: the app's window is disabled while the picker is up and brought back to the front
  afterwards; the picker grants it the right to do so.

## Testing it without registering anything

```
cargo build -p shellman-files -p shellman-pickhook --examples
cargo run -p shellman-pickhook --example host -- open      # also: many save folder veto
cargo run -p shellman-pickhook --example host -- missing   # picker missing: the real dialog
cargo run -p shellman-pickhook --example host -- failing   # picker fails: the real dialog
cargo run -p shellman-pickhook --example host -- custom    # custom controls: the real dialog
cargo run -p shellman-pickhook --example host -- actctx    # the registration-free experiment
```

The host gets its dialog from the hook's class factory directly, never from the registry.
