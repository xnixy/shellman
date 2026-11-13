# shellman

A replacement for everything `explorer.exe` does, in one visual style: Windows 11 glass, one
accent color, fast.

## Pieces

| App | Replaces | Status |
| --- | --- | --- |
| `apps/files` | File Explorer windows | working: tabs, list/grid, thumbnails, shell menu, auto-refresh, Ctrl+H hides or shows hidden files, the Recycle Bin as a place (`--recycle-bin`). No settings UI |
| `apps/bar` | taskbar, tray, clock, Start button (as a top bar and a dock) | working: a top bar and a dock on every monitor, tray, pins, fullscreen step-aside. Next: hover previews, Win keys |
| `apps/launcher` | Start menu and search | working: Spotlight-style panel, app grid, ranked search, most used, the Win key (through the switcher's hook) |
| `apps/switcher` | Alt+Tab, Task View | working: Alt+Tab with live previews. Next: Task View (Win+Tab) |
| `apps/desktop` | wallpaper window | working: wallpaper per monitor; keeps Windows' modern-app host running; hands a right-click to the shelf |
| `apps/shelf` | desktop icons, the desktop's right-click menu | stage 1 working: the shelf (drop zone, Desktop / Downloads / Recent stacks that fan open, drag out, drop in), the right-click menu, Settings › Desktop. Next: widgets top right (stage 2) |
| `apps/tasks` | Task Manager (Ctrl+Shift+Esc, through the switcher's hook) | working: Processes (apps and background, grouped, sort, search, End task / End process tree / details), Performance (CPU, memory, GPUs, disks, network), Services (read-only: status, startup type, PID, description, sort, search, go to process; read through the SCM in `crates/core/src/services`, off the UI thread, only while the page shows). End task / tree try unelevated first and, only when Windows refuses, go through `shellman-elevate`'s `end-processes <pid>:<start time>...` behind the UAC prompt (the helper rechecks start time and refuses critical processes); critical processes stay un-endable. Reads through `crates/core/src/procs` |
| `apps/terminal` | Windows Terminal | stage 1: one shell in glass, GPU-drawn grid, scrollback. Next: tabs, selection, mouse, profiles |
| `apps/settings` | Settings pages for the shell | working: Appearance, Desktop, Dock, Top bar, Launcher, About; rebuilt Windows pages: Sound, Display, Apps, Startup, Fonts, Mouse & keyboard, Power, Date, time & region, Storage, Default apps (changes through Windows Settings), Windows Update |
| `apps/session` | the shell process itself | built, not yet the shell: starts and supervises the apps, runs startup items, safety net, `--install`/`--uninstall` (never run yet) |

Shared code:

- `crates/ui`: theme (palette from one accent), glass window frame, widgets.
- `crates/core`: shell plumbing that has no layout: file ops, shell images, the Windows context
  menu, folder watching, the app index (the shell's Apps folder), live DWM previews
  (`live_thumb`), shared settings types (`settings::{dock,bar,launcher}`), and settings files
  under `%APPDATA%\shellman`. `SHELLMAN_CONFIG_DIR` overrides that folder, for tests.
- `crates/icons`: shellman's own app icons, drawn in code. The build-time `.ico` generator in
  `tools/icongen` and the running apps share it.

Each app is its own process, so a crash in one never takes down the bar. `session` starts
and restarts the others.

## Ground rules

- Fast first: nothing O(n) per frame, no blocking I/O on the UI thread, virtualize long lists.
- Small, readable files: one concern each, a few hundred lines at most.
- Everything is built and tested **next to** the real Explorer. Only the last step, `session`,
  swaps the shell, and that swap is a single registry value that can be set back.

## Bar plan: a top bar and a dock

Rule: **the top bar is the system, the dock is apps.** Both are windows of one process,
`apps/bar`, since they share the window list, hooks and icon cache.

| Explorer job | Where |
| --- | --- |
| Start / launcher | top bar, left corner; first icon in the dock (Win key later) |
| Running and pinned apps, switching windows | dock |
| Tray icons, all of them | top bar, right, on every monitor (up to 8, the rest behind a chevron) |
| Network, volume, battery | top bar, one status pill |
| Clock, calendar, notifications | top bar, right corner |
| Files, Recycle Bin | dock: Files after the launcher, the bin at the end |
| Space for maximized windows | each monitor reserves its top bar and dock band |

**Top bar**: 32 pt, full width, flush to the edge so edge pixels hit items.
- Left: the launcher glyph in the corner, then the active app's name and window title.
- Centre: empty, except a "now" pill for media, copy progress, or an app asking for attention.
- Right: tray, overflow chevron, status pill (scroll for volume), clock ("Wed 11 Nov  14:32").
  The clock wakes once a minute on the boundary, plus on time changes and resume.
- The tray appears on every monitor's top bar by default (`tray: all | primary` in `bar.json`).
  `Shell_NotifyIconGetRect` answers with the icon's place on the monitor where it was last
  hovered or clicked (else the primary's), and a click's anchor is that icon's bottom middle
  there, so an app's menu opens on the bar that was used.

**Dock**: bottom centre, a floating glass pill.
- Size: a setting, small / medium / large icons (32 / 40 / 48 pt). Cell = icon + 12 pt;
  height = cell + 4 pt (medium: 52 pt cells, 56 pt tall). Reserved space; autohide is a setting.
- Corners: Windows' own 8 pt window radius. DWM paints glass over the whole window and ignores
  window regions (measured), so a larger radius would mean giving up real blur.
- Order: Launcher, Files, pinned apps, divider, running unpinned apps, divider, Recycle Bin.
  Launcher, Files and the bin are fixed.
- Every dock shows all apps.
- Running dots (up to three for several windows); an accent bar marks the app in front;
  attention pulses at most three times.
- Click activates. Clicking an app already in front with several windows steps through them in
  a fixed order (by handle), and so does scrolling. With one window, a click on the app in front
  minimizes it. Middle click opens a new window.
- Hovering shows live DWM previews, and sliding along the dock scrubs them.
- Right-click on an icon: a native Windows popup with Pin / Unpin and Close. On the Launcher
  icon: Settings. On the Recycle Bin: Open (in Files, never Explorer) and Empty Recycle Bin
  (grey when empty; a native confirmation first). The dock window is only as tall as its icons, so the menu cannot live in it.
- No magnification, which would repaint on every mouse move; a short hover lift instead.
- Holding Win shows 1-9 on icons. Dropping a file on an icon opens it with that app.
- Slots for progress and overlay badges; Files reports its copy progress directly.
- Tray icons always stay in the top bar, in stable places; they are never folded into the dock,
  because many Windows apps close to the tray and their icon would hop between bars. When a tray
  app that also has a dock icon shows something new (an unread count, an alert), its dock icon
  mirrors it as a badge.

**Behaviour**
- Bars never take focus (`WS_EX_NOACTIVATE`), so "minimize if in front" sees the real
  foreground window.
- Fullscreen apps and games: decided per monitor from the foreground window. It must cover the
  whole monitor (a maximized window with a title bar does not count), and it must not be a shell
  or desktop window (including class `ShellmanDesktop`). The bars on that monitor step aside.
- Hidden bars are woken by a posted message, because hidden windows get no frames.
- The front window's moves are heard through a hook on its thread only.
- Space: each monitor reserves its top strip and a dock band (dock height + two 8 pt gaps),
  measured from the monitor edge, or from what Explorer grants when it runs. Never measured from
  a work area that already includes shellman's own reservation. Next to Explorer this goes
  through `SHAppBarMessage`; as the shell, by setting the work area per monitor directly.
- Monitors: a top bar and a dock on every monitor by default (`docks: all | primary` in
  `bar.json`). The tray likewise (`tray: all | primary`).
- Window list from `RegisterShellHookWindow` plus narrow WinEvent ranges, never polling.
- Tray: copy each icon (owners destroy theirs), coalesce animated updates, check that clicks reach
  elevated apps.

**Architecture**
- The bar's main window is a cloaked host. Every top bar and dock is a deferred viewport keyed by
  monitor, placed with Win32 in physical pixels.
- `--no-shell` runs a test bar that reserves nothing and hosts no tray.

## Launcher

- Resident hidden process. Running the exe again signals it (named mutex + event), so a click is
  a toggle (~10 ms).
- Centred glass panel, large search field.
- Empty query: app grid, with a "most used" row from its own launch counts.
- Typing: ranked list (exact, prefix, word start, substring, fuzzy; ties by launch count), with
  "Run <query>" last.
- Apps come from the shell's Apps folder and launch via `shell:AppsFolder\<id>`. Without Explorer
  that folder never learns of newly installed apps, so the index also reads the per-user and
  common Start menu `.lnk` files (target resolved with `IShellLink`, skipped when the Apps folder
  has the name or target, same noise rules), launching those by path. shellman's own apps (Files,
  Settings, Task Manager, Terminal: the `shellman-*.exe` beside the running binary, when present)
  are built in, rank before a same-named Windows app, are also found by "shellman", and the
  Windows namesake is labelled "Windows".

## Switcher

- Own resident process. A low-level keyboard hook runs on its own thread, with pure decision
  logic.
- Takes only Alt+Tab and Alt+Shift+Tab, plus Esc, Enter and arrows while open. Alt's release
  always passes through.
- The same hook turns a lone Win tap into the launcher while Explorer is not the shell: Win's
  press always passes, only a lone tap's release is replaced by 0xE8 and the release.
- Hidden between uses. While Alt is held it is awake but cloaked, and it shows after 120 ms or on
  the second Tab, so a quick tap never flashes.
- One press of an unused key (0xE8) wins the foreground and stops apps from opening their menu
  bar.
- Known limits: elevated windows; always-on-top windows sort first.

## Desktop

- Plain Win32 window per monitor (class `ShellmanDesktop`) at the bottom of the z-order. One kept
  bitmap each. No egui.
- `IDesktopWallpaper` needs Explorer. As the shell, it reads the classic settings instead: one
  picture for all monitors, no slideshow.
- No icon grid. The design (`docs/design/desktop-concepts.html`, "Shelf + Widgets") puts a shelf
  on the left edge and, later, two widgets top right; "Clean" (wallpaper only) is the other style.
  Settings in `desktop.json` (`settings::desktop`): style, shelf on/off, edge, stacks, where
  dropped files go, and the widgets' switches (read by nothing yet).

### The shelf: its own process

Decision: what sits on the wallpaper, and the wallpaper's right-click menu, live in a new app,
`apps/shelf` (`shellman-shelf.exe`), started by the session after the bar (and by the desktop
when no session runs). Not in the desktop process: that one carries Windows' modern-app host and
must stay plain Win32; the shelf loads egui and the GPU, shell thumbnail handlers and OLE drag and
drop, any of which can crash, and the session restarts a crashed shelf without touching the
wallpaper or the host. Not in the bar: the bar is all always-on-top windows and owns the tray, and
a shell extension crashing in a thumbnail must not take the tray down. The cost is one process
(~130 MB working set with its GL surfaces, nothing at rest). What it shares with the bar and Files
is in the crates: `glass`, `thumbs`, `transfer` (drag source, drop target, clipboard), `fsops`,
`watch`, `winlist::fullscreen` and the window watcher, `outside` (the click-outside hook, moved out
of the Control Center).

- One window, the root viewport: on the primary monitor's edge, as tall as the monitor, wide
  enough for an open fan; only its islands show and take clicks (`set_islands_only`).
- Desktop level: a tool window that never activates, so never in Alt+Tab or the dock. Whenever
  anyone moves it in the z-order (showing it, the window library), the move is redirected to just
  above the topmost visible desktop window (`ShellmanDesktop`, `Progman`), found walking from the
  top: other apps keep windows under the wallpaper (an "Open With" helper was found there), so the
  first non-desktop window from the bottom is the wrong place. Other windows coming forward never
  move it, so this costs nothing at rest. While a stack is open the window is topmost and has the
  focus (for Esc); it goes back down when the fan has closed.
- Steps aside for fullscreen apps like the bars: the window watcher posts a check, and the shelf
  shows no islands while `foreground_covers_monitor_of` says so.
- Idle: no repaint loop. Frames come from the pointer, folder changes (a folder watch per folder,
  reads on worker threads), thumbnails arriving, the settings file, and the menu request; the fan's
  160 ms open/close animation is the only animation. Measured: 0.016 s of CPU in 10 s at rest.
- Stacks: Desktop (the user's and the common Desktop), Downloads, Recent (the Recent folder's
  shortcuts resolved, files that still exist, each once). Newest first by the later of created
  and modified; hidden/system files and partial downloads left out. A fan shows the six newest and
  "Open <folder> in Files · N more". Items drag out as a real OLE drag (as from Files).
- Drops: files dropped on the shelf go into the chosen folder through `fsops::shell_op` (progress,
  conflict dialog, undo), with Explorer's rules: a move on the same volume, a copy across volumes,
  Ctrl/Shift/Alt as usual. A browser's picture or text becomes a new file; virtual files (a
  browser's `FileGroupDescriptor` without a path) are refused.
- `SHELLMAN_SHELF_FOLDERS=<dir>` puts Desktop, Downloads, Recent and Documents under one folder
  (tests); `SHELLMAN_WALLPAPER_DRYRUN=<file>` writes the chosen wallpaper there instead of setting
  it; `SHELLMAN_FORCE_LIGHT` / `SHELLMAN_FORCE_DARK`. Log: `%LOCALAPPDATA%\shellman\shelf.log`.

### The right-click menu

The desktop process hears the right-click on a wallpaper window, lets the shelf's process take
the foreground (it received the click, so it may), and posts the shelf window a registered message
with the point (`core::desktop_menu`; `--menu x y` starts a shelf that opens it when none runs).
The shelf shows a plain Windows popup there, the component the dock's menu uses: New (Folder,
Text document, Markdown note), Paste (greyed with nothing to paste), Open in Files, Open Terminal
here, Change wallpaper…, Desktop settings, Display settings. "Edit desktop…" comes with the widgets.

A first version drew shellman's own menu on glass in an egui window as large as the clicked
monitor. Opening it repeatedly crashed `dwm.exe` (12 Nov, 18:00:02, `dwmcore.dll`, E_INVALIDARG),
most likely from a glass panel created and destroyed per menu on a monitor-sized window (and, in
one state, re-attached every frame); a monitor-sized topmost window also made the bar hide as for
a fullscreen app. Rule from it: never a window as large as a monitor for a popup, and never
create glass per click. A glass menu should come back as one window, sized to the menu, made once
and kept.

`dwm.exe` crashed 16 times that day (10:24, 12:06, 18:00), always the same fail-fast in
`dwmcore` after `CreateTexture2D` returned E_INVALIDARG: a texture of size 0, negative or past
16384. Glass now defends itself (`glass::guard`, the one choke point before Windows.UI.Composition
and the backdrop window): every side 1 to 8192 px (below 1 the piece or backdrop window is
hidden, never sized to 0; above, clamped), offsets clamped, corner radius at most half the
shorter side, shadow blur and offset clamped, a scale of 0/NaN/inf taken as 1; pieces are made
hidden and their shadow shows only after the mask surface has its size; a panel rebuilds at
most once per 500 ms and never during a rebuild; a thread makes at most 8 backdrop windows in a
burst, then 3 a second (more attaches get no composition and paint their own surface).
`examples/glass_stress.rs` drives a panel through 0x0, 1x1, inverted and absurd shapes, NaN
islands, minimise, hide, resize to nothing, a DPI message, 20 refreshes and 30 attaches in half
a second; run on a secondary monitor it left no dwm event. The rule for callers stays: no
monitor-sized glass popups, no glass made per click or per frame.

"Open Terminal here" is one function (`shelf::terminal`), to be replaced by shellman's own
terminal: Windows Terminal through its app alias `wt.exe -d <folder>` (starts without Explorer,
tested), else the activation manager, else PowerShell.
## Settings

- One app writes the shared files under `%APPDATA%\shellman`. Every app follows them through its
  folder watch.
- No OK / Apply: a change applies as it is made.
- Opened from the top bar glyph's right-click, the dock Launcher icon's menu, and the launcher's
  "shellman Settings" row.
- Single instance.
- Windows pages rebuilt here (Windows' own Settings does not open without Explorer): all read the
  live value when shown and write at once, from `crates/core/src/system/` (`pointer`, `power_*`,
  `region*`, `storage`, `default_apps`). Measured unelevated on 26300: mouse and keyboard
  `SystemParametersInfoW`, the plan's timeouts and button actions, power mode and plan
  (`powrprof`), the time zone (`SeTimeZonePrivilege` is held by Users, switched on for the call),
  daylight saving and the `HKCU\Control Panel\International` values all work; nothing needs
  `shellman-elevate`. Rows a machine lacks are hidden (no lid, no battery column on a desktop).
  Default apps: "Change" lists the apps that claim the type (`RegisteredApplications`) and opens
  Windows Settings on the picked one (`ms-settings:defaultapps?registeredAppMachine=` /
  `registeredAppUser=` / `registeredAUMID=`, all three seen working on 26300); Settings writes the
  choice, protected types too, and the page follows `FileExts`/`UrlAssociations` with
  `RegNotifyChangeKeyValue`. Without the host the page offers the modern-apps switch instead.
  Measured on 26300 without Explorer and not usable: `IAssocHandler::MakeDefault` (S_OK, writes
  nothing, even for a made-up type), `SHOpenWithDialog` with the register flags (only a notice
  "go to Settings > Apps > Default apps"), `OpenWith.exe` / `OpenAs_RunDLL` (start, show nothing:
  the flyout is immersive and needs Explorer, also with the host on); a deep link with the host off
  fails at once (0x80040900). `UCPD.sys` (4.9.0.720417) guards `UserChoice`/`UserChoiceLatest` of
  `http`, `https`, `.pdf`, `.htm(l)`, `.doc(x)`, `.xls(x)`, `.ppt(x)`. shellman never writes
  `UserChoice`, never computes its hash and never touches UCPD.
  Storage counts folders on workers, never follows reparse points, and only offers "Open in Files".
- Windows Update talks to the Update Orchestrator through `Windows.Management.Update`
  (`system::updates`; the second contract, missing from the `windows` crate, is generated into
  `system::wu_api` and cast at the call, so older builds fall back to the first). Never WUA for
  downloads or installs; WUA only for the history. A worker holds the manager while the page
  shows and forwards Windows' events. Measured unelevated on 26300: "Check for updates"
  (`StartScan(true)`) works and Windows then downloads and installs by itself; "restart needed"
  comes from the orchestrator (`ReadyToReboot` / `Commit`), not WUA or the registry. Pause is
  `UX\Settings` `Pause*` values (names found in `MoUsoCoreWorker.exe`; the next scan sets
  `UpdatePolicy\Settings` `Paused*Status` and offers nothing; deleting them resumes). Active
  hours and "latest updates as soon as available" are left out: written values changed nothing
  Windows reports. Approving an optional update is built but untested (only drivers were offered),
  so there is no elevate op for it. "Update and restart" is `power::update_and_restart`
  (`InitiateShutdownW` restart + install updates; never run live); the Control Center's power
  menu can offer it on the same signal (`updates::Snapshot` says a restart is needed).
- Brightness (Display page and Control Center, one `brightness::BrightnessWatch`): a monitor with
  HDR on gets "Brightness (HDR)", Windows' SDR content brightness (`display::sdr`): read with the
  documented `GET_SDR_WHITE_LEVEL`, set with the private `DisplayConfigSetDeviceInfo` type -18
  (header + level + final flag, 1000..6000 = 80..480 nits, the slider's 0..100 as in Windows).
  Windows stores it itself (`MonitorDataStore\<monitor>\SDRWhiteLevel`). HDR off: DDC/CI or the
  backlight as before (HDR monitors such as the ASUS XG32 OLEDs ignore DDC brightness in HDR).

## Modern apps without Explorer

Settings, Calculator, the Store and every `ms-settings:` link are immersive (CoreWindow) apps.
Without Explorer they cannot open: activation fails with 0x80040900 after ~45 s, `ms-settings:`
fails with error 1155, `shell:AppsFolder\<id>` with 0x80040154. Packaged Win32 apps (Notepad,
Photos, Terminal) are fine. What is missing is the immersive shell; faking Progman, the shell
window or shell-ready events does nothing.

Decision: run Windows' own host, `%SystemRoot%\System32\ShellAppRuntime.exe` (the app half of
Shell Launcher v2), from `shellman_core::app_host`, carried by the desktop process (plain Win32,
the longest lived and first started app). Measured on 26300 (build 10.0.26100.9818 of the exe):

- Started with no arguments, unelevated, with no shell window, it owns the shell window, the
  task window and the immersive-shell windows in 0.2 to 6 s, creates no `Shell_TrayWnd` (the bar
  keeps the tray), and modern apps open in ~0.3 s. Its Progman stays under shellman's desktop.
- It exits at once if a shell window or another copy exists.
- A clean stop is WM_QUIT to the shell window's thread: gone in 0.1 to 8 s, nothing restarts.
  Its exit code is 1 either way, so a crash is told from a stop by Winlogon: after a kill or
  crash it starts Explorer as the shell (via userinit) within 0.2 s; after WM_QUIT it does not.
- It takes the task-window role; the bar's own claim is then refused, minimized windows are still
  hidden, and when the host ends the role is empty, so the supervisor gives it back to the bar.
- It starts StartMenuExperienceHost, SearchHost and TextInputHost (~116 MB); Win alone would open
  Windows' Start (see the switcher's `winkey`). Win+A opens Quick settings; Win+R, Win+V, Win+S,
  Win+N, Win+Shift+S open nothing; Win+Tab only focuses an empty staging window.
- `IDesktopWallpaper` is still unavailable with it running.
- Blur does not need it: DWM only needs the transparency state its immersive shell publishes,
  and the session publishes that itself (see "Transparency" under the session). With the host
  on, the host owns the state and the session leaves it alone.

The supervisor starts it when nobody owns the shell window and no Explorer runs, waits on its
process, and when Winlogon replaces it with Explorer stops that one Explorer (started after the
host died, by userinit), re-announces the tray (`TaskbarCreated`) and starts the host again; three
failures in ten minutes and it gives up. A clean stop is left alone. shellman never claims the
shell window itself. On shellman's exit the host is left running: harmless, and ending it any
other way than WM_QUIT would bring Explorer back. "Keep Settings, Calculator and the Store
working" (About page, `shell.json`, default off) turns it off; `SHELLMAN_APP_HOST=off|on` overrides
for tests. Its log is `%LOCALAPPDATA%\shellman\app-host.log`.

The host runs the user's startup items (Run keys without StartupApproved, packaged startup
tasks, Startup folders) when it is the first to create the logon's `RunStuffHasBeenRun` marker;
the session creates that marker itself when it runs them, so the host never repeats them (see
"Startup items" under the session).

The bar puts its work areas back when they change under it: an Explorer that runs even briefly
sets every monitor's for its own taskbar, and again whenever they change while it runs. So the
bar waits: a work-area change or `TaskbarCreated` starts a timer; after 400 ms of quiet, if no
`explorer.exe` owns a `Shell_TrayWnd` (by process: the bar owns one too), its work areas are put
back, otherwise it looks again every second. At most five rounds in 30 s, then a minute's pause.
`SHELLMAN_WORKAREA_LOG=<file>` traces it.

If the exe is missing or does not take the shell window, nothing else changes: modern apps stay
unavailable and the Control Center hides its Settings links, except where a Control Panel applet
opens without Explorer (Bluetooth: `bthprops.cpl`; Sound: `mmsys.cpl`; also `main.cpl`,
`sysdm.cpl`). Never `ncpa.cpl`, `control.exe` or Power Options (they start Explorer) or
`desk.cpl` (it opens Settings).

Not yet tried with the host running: toasts, the touch keyboard, snap layouts, sleep and resume,
display changes, long uptime.

### A host of shellman's own (tried; cannot work)

Goal was to replace ShellAppRuntime with a small shellman process doing only the part that brings
up the immersive shell, with ShellAppRuntime as the fallback. It cannot be done: Windows reserves
what the immersive shell needs to Windows' own signed binaries. The experiment is kept off the main
line, on branch `apphost-experiment` (`apps/apphost`).

What ShellAppRuntime 10.0.26100.9818 does for it (read from the exe; twinui.pcshell 9974):

- STA main thread: `CoInitializeEx(APARTMENTTHREADED | DISABLE_OLE1DDE)`; `IGlobalOptions`
  exception handling DONOT_HANDLE_ANY and RO settings 0x218.
- `CoInitializeSecurity(absolute SD, -1, NULL, NULL, AUTHN_LEVEL_DEFAULT, IMP_LEVEL_IDENTIFY, NULL,
  EOAC_NONE, NULL)` with `O:BAG:BAD:(A;;0x3;;;PS)(A;;0x3;;;AC)(A;;0x3;;;SY)(A;;0x3;;;BA)` plus
  `(A;;0x9;;;S-1-5-90-0-<session>)` (the DWM user).
- Legacy init: AUMID `Microsoft.Windows.Explorer`, BAM ioctl, `EnableMouseInPointer(0)`, shell32
  #899(3), #188(1) when configured, `CoInitializeEx` again, shell32 #904(1, 1).
  `SetProcessShutdownParameters(3, NORETRY)`; `OleInitialize` when configured.
- Creates an object (IID {6597eb02-ceb7-4c90-a0a6-e3176d67e179}) kept for the shell's lifetime.
  Then "Progman" / "Program Manager", `WS_EX_TOOLWINDOW`, `WS_POPUP | WS_CLIPCHILDREN`, the virtual
  screen (its WM_CREATE claims `SetShellWindow`); then the taskman window (WM_CREATE: ARW_HIDE
  minimized metrics, `SetTaskmanWindow`, `RegisterShellHookWindow`; each SHELLHOOK message goes to
  `CLSID_ImmersiveShell` {c2f03a33-...} local server -> `IServiceProvider::QueryService(
  {4624bd39-5fc3-44a8-a809-163a836e9031}, {914d9b3a-5e53-4e14-bbba-46062acb35a4})` slot 5
  (wParam, lParam), except code 0x32; HSHELL_APPCOMMAND also to its audio component).
- shell32 #938(), then `CoCreateInstance({c71c41f1-ddad-42dc-a8fc-f5bfc61df957}, INPROC,
  {1c56b3e4-e6ea-4ced-8a74-73b72c6bd435})` (twinui.pcshell's builder), slot 3 ->
  `IImmersiveShellController` {d6948331-eaf5-4365-86ac-d8d1d03b4600}, slot 3 `Start()`.
- After Start: logon tasks and `Local\ShellStartupEvent`, tray window, startup apps,
  `RegisterApplicationRestart`, `ShellReadyEvent`, then a message loop until WM_QUIT; slot 4
  `Stop()` on the way out.

Why it fails (10.0.26300, verified):

- `shellman-apphost` gets through the builder, the controller, the shell window, the task window
  and the shell hook. `Start` then fails fast with `IMSC_E_SHELL_COMPONENT_STARTUP_FAILURE`
  (0x80270233), with or without the shell hook, task window, full-size Progman, AUMID and shell32
  ordinals (two runs: with all of them, and with none; the record below is from the first).
- Before failing fast twinui records the failure under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\ImmersiveShell\Startup` (`PreviousFailure_*`;
  the next Explorer clears it within a second): `pcshell\twinui\appmanager\lib\appmanager.cpp`,
  line 5032, `0x80070005` (access denied). That line creates the application manager's
  `ApplicationManager_ImmersiveShellWindow`, with `CreateWindowInBand(.., band 12)`
  (ZBID_IMMERSIVE_BACKGROUND).
- win32k refuses every band but the default, the desktop and band 16 to an ordinary process
  (`shellman-apphost --band-test`: ERROR_ACCESS_DENIED for bands 2 to 14 and 17). The binaries
  that may use them carry an `.imrsiv` PE section and are Windows-signed (explorer.exe,
  ShellAppRuntime.exe and about fifty other System32 executables). With an `.imrsiv` section of
  its own but no Windows signature the host is refused all the same. So the gate is "a
  Windows-signed image marked immersive": the end of this road. Getting past it would mean
  running inside one of those binaries or defeating the signature check, neither of which a
  shell should do.
- None of the other differences from ShellAppRuntime (OLE, the {6597eb02} object, the start-menu
  cache, logon tasks) matter: the failure is the first band window, before any of them would.

What a failed `Start` costs, for whoever tries a host again: twinui fails fast
(`RaiseFailFastException`, no context), and kernelbase ends the process through a hard error
without running vectored handlers or the unhandled-exception filter, so the host cannot give up
the shell window first; Winlogon then starts Explorer within 0.3 s. twinui fails fast only the
first time a given failure happens (it compares with the recorded one) and skips the component on
a repeat, unless a debugger is attached; since the next Explorer clears the record, every attempt
fails fast.

Decision: no host of our own is possible, and Windows' host is off unless the user turns it on
(see above). Settings pages people use most are to be rebuilt in shellman Settings.

## Terminal

`apps/terminal` (`shellman-terminal.exe [command line]`) is to replace Windows Terminal. Stage 1:
one shell (default `powershell.exe`; the rest of the command line, as typed, runs instead) in a
title pill and one glass island. Not yet: tabs, splits, selection and copy, mouse reporting to
programs, links, profiles, settings, IME, ligatures, colour emoji, default-terminal registration.

- **Emulation**: `alacritty_terminal` 0.26: grid, scrollback (10 000 lines), VT parser and the
  pseudo-console (ConPTY, or a `conpty.dll` found on PATH or beside the exe), on a thread of its
  own. Its listener answers the pty directly (cursor reports) and wakes the window.
- **Structure**: `session.rs` is one shell (pty + emulator) and knows nothing of drawing;
  `snapshot.rs` turns its visible grid into plain cells (colours resolved through `palette.rs`,
  inverse, faint, wide characters, the cursor); `render/` draws cells and knows nothing of the
  emulator. Tabs later are several sessions and one view. Keys to bytes are pure and tested
  (`keys.rs`): xterm sequences, application cursor mode, Ctrl and Alt, bracketed paste.
- **Drawing**: a GPU renderer inside egui's frame (an `egui_glow` paint callback; egui still
  draws the chrome and `shellman_ui::glass` the glass). Glyphs are rasterized once by DirectWrite
  (`crossfont`, with Windows' font fallback) at the monitor's pixel size into a 2048 px greyscale
  atlas; cells snap to whole pixels. Three instanced draws: background runs, glyphs, then
  underline / strike / thin cursor. Each grid row has fixed slots in the instance buffers and a
  hash; only rows whose hash changed are rebuilt and sent. Font: Cascadia Mono, else Cascadia
  Code, else Consolas, 12 pt.
- **Glass and colour**: cells with the default background draw nothing, so the island is the
  same glass as every other app and follows the glass setting live (the app watches
  `appearance.json`). Explicit backgrounds are opaque, so full-screen programs look as they
  were made. Text is greyscale antialiased: DirectWrite's ClearType coverage is averaged, since
  subpixel coverage cannot be blended right over glass whose colour only DWM knows. Default text
  and the cursor follow the theme (a black accent falls back to the text colour); the 16 colours
  are Campbell on dark and a darkened set on light.
- **Repaint**: only on new output, input, resize or a theme change. The cursor does not blink.

Measured (release, 4K at 150%, maximized, 240 Hz monitor, `SHELLMAN_TERMINAL_STATS=<file>` logs
frame times):

- `cmd /c dir /s C:\Windows\System32`: 3.4 to 4.8 s (median 3.8), Windows Terminal 3.0 to 4.2 s
  (median 3.5), five runs each. `Get-ChildItem C:\Windows\System32 -Recurse`: 21.5 and 18.4 s,
  Windows Terminal 19.5 and 17.9 s. So within about 5 to 10 %; Windows Terminal ships a newer
  ConPTY (OpenConsole) than the system's, which may be the rest.
- While that output scrolls: a frame every vsync (241 fps), CPU per frame 0.5 to 0.8 ms on
  average, 2.3 ms at most; rebuilding and hashing the rows 0.3 to 0.45 ms of it; about 60 rows sent
  a frame.
- Idle, with the window focused or not: 0 ms of CPU in 20 s.
- First text on screen 0.7 s after start.

## Session

`apps/session` (`shellman-session.exe`) is the one process Windows starts as the shell. Plain Win32,
no egui, no `shellman-core`; it must be the most robust piece. Beside Explorer nothing starts it.

- **Starts** next to its own exe, in order: desktop, bar, shelf, switcher, launcher `--background`,
  control `--background` (250 ms apart). Every child gets `SHELLMAN_SESSION=1`. The bar then leaves
  the residents to the session; started by hand it still starts them itself.
- **Supervises** with one `MsgWaitForMultipleObjectsEx` on all process handles plus the window's
  messages (no polling). Exit code 0 means ended on purpose (the apps have no quit UI yet, and a
  `--background` start that finds its resident exits 0): not restarted, but another running copy
  is adopted if there is one. Any other code is a crash: restart after 0.5 s, 2 s, then 8 s; five
  crashes within a minute and the app is given up on. All decisions are in `policy.rs`, pure and
  tested. Log: `%LOCALAPPDATA%\shellman\session.log` (64 KB, rotated).
- **Single instance**: mutex `Local\shellman-session[-<SHELLMAN_INSTANCE>]`.
- **Adoption**: at start, an app whose exe already runs in this logon session is held, not
  started again (a session that crashed and was restarted by Winlogon, or a bar started by hand).
- **Logoff**: a hidden top-level window answers WM_QUERYENDSESSION with yes at once; on
  WM_ENDSESSION (or WM_CLOSE) it posts WM_CLOSE to every top-level window of the children, waits up
  to 4 s, logs who is left, never kills.
- **Startup items** (what Explorer does at logon): RunOnce (HKCU; HKLM only if elevated, the value
  is deleted before the command starts, or after with a `!` prefix), Policies\Explorer\Run, HKLM
  Run and its WOW6432Node twin, HKCU Run, then packaged apps' startup tasks, then the common and
  the user Startup folders (via ShellExecute). First start 6 s after the session, then 1.5 s
  apart. `shellman-session --dry-run` prints the apps and each item with why it is enabled or
  disabled, and Windows' run marker (below), and changes nothing.
  - Switched-off Run and folder items: `...\Explorer\StartupApproved\{Run,Run32,StartupFolder}`,
    read as the shell's own `shell\lib\runonce` code does (ShellAppRuntime.exe 10.0.26100.9818,
    which shares it with Explorer): only the low bit of the first DWORD counts (odd disabled,
    even or 00 enabled), the FILETIME is ignored (`01 00..00` is disabled), missing or unreadable
    means enabled, and an item is judged only in its own hive's key (HKLM items under HKLM).
    Explorer's behaviour agrees (Shell-Core/Operational 9707 events: it never ran Docker Desktop's
    `01` item). See `startup/approved.rs`.
  - Packaged startup tasks (`desktop:`/`uap5:StartupTask` extensions, `windows.startupTask`):
    listed from each package's manifest, judged by
    `HKCU\...\AppModel\SystemAppData\<family>\<TaskId>\State` (`StartupTaskState`: 0
    disabled, 1 disabled by user, 2 enabled, 3 disabled by policy, 4 enabled by policy; missing:
    the manifest's `Enabled`). Started the way the shell does, through the undocumented but
    unrestricted `Windows.ApplicationModel.Internal.StartupTaskInternal` (list of the user's
    enabled tasks) and `...StartupTaskActivator` (activate one; the app runs with its identity,
    launched by sihost), matched by program name. UWP (non full-trust) tasks are skipped while
    modern apps cannot open. See `startup/packaged*.rs`. Not verified end to end on this machine:
    its only enabled task (1Password) already ran; listing is.
  - Once per logon, and only by shellman: a volatile HKCU key `Software\shellman\session-startup`
    (it outlives a crashed session and is removed at logoff; a named kernel object would die
    with the session); a session that adopted apps skips them as well. It also creates Windows'
    own marker, volatile `HKCU\...\Explorer\SessionInfo\<session>\RunStuffHasBeenRun`:
    Explorer and ShellAppRuntime run the Run keys, startup tasks and folders only when they
    create that key themselves. Without it, turning on "Keep Settings, Calculator and the Store
    working" mid-session made the host run everything again (seen: a second
    SecurityHealthSystray, Docker Desktop, and 1Password's task), and the host skips
    StartupApproved for its Run pass (its filter is never configured: the configure call goes to
    `IUnknown_QueryService(NULL, ..)`), so switched-off items came up too. If the marker already
    exists on a fresh logon, Explorer or the host has the run and shellman does not run them.
    The host still processes HKCU RunOnce each time it starts. Not started in test runs.
  - Other places, for the record: RunServices(Once) are obsolete and run by nothing on Windows
    10/11; Active Setup (`HKLM\...\Active Setup\Installed Components` StubPath) is run by
    Explorer's logon tasks (not handled; only matters right after installs that use it);
    `Winlogon\Userinit` runs before any shell (Winlogon's own); `HKCU\...\Windows\Load`/`Run`
    (win.ini era) are honoured by Explorer only on old builds; scheduled tasks "at log on" are
    started by Task Scheduler, not the shell, so they run without Explorer already.
- **Safety net**: if Shift is held at the session's start, or the bar's exe is missing, or the bar
  was given up on (crash budget spent) or ended with exit code 0, or a running bar has no
  `Shell_TrayWnd` 20 s after its own latest (re)start, the session starts `explorer.exe` (unless
  one runs), logs why, and stops supervising (it stays alive, so Winlogon does not restart it in a
  loop). A bar whose restart is pending is never "lost" (a single crash after 80 minutes used to
  hand the session to Explorer, because the 20 s were counted from the session's start). If
  Explorer has not become the shell 15 s after a fallback (the modern-app host owns the shell
  window, so it only opens a folder window), supervising resumes once with a fresh crash budget;
  a second fallback stays parked. Never after Shift. Shift also skips every shellman app.
- **Transparency (blur)**: DWM draws every blur (the glass's host backdrop, acrylic, Mica) flat
  unless the session WNF state `0x0F950324A3BC4035` (family IMSN, probably
  `WNF_IMSN_TRANSPARENCYPOLICY`; read from uDWM's `CWindowList` on 10.0.26300) holds a non-zero
  u32. Windows' immersive shell (`twinui.pcshell`, in Explorer or ShellAppRuntime) publishes it;
  without either it has no data and everything is flat. Its security descriptor lets INTERACTIVE
  users publish, so the session does (`core::system::transparency`, `NtUpdateWnfStateData` with
  no type id, the current session's scope, no stamp check): at its start, before any app, with
  the user's `HKCU\...\Themes\Personalize` `EnableTransparency` (default 1). It queries first and
  writes only when the state is empty or differs, never while Explorer or ShellAppRuntime owns the
  shell window (or the immersive shell's window exists): that shell owns the state then. A thread
  follows the setting's key, that shell's end, and the session window's `ImmersiveColorSet` and
  `shellman-glass-refresh` messages; after a publish that changed the value it broadcasts
  `shellman-glass-refresh` (a backdrop made while DWM said no stays flat). One session-log line
  each time (`transparency (start): published 1 (stamp 0 value none -> stamp 1 value 1)`, or the
  NTSTATUS). The kernel keeps the data until the logon ends, so blur survives the host stopping.
  Only the real session; a test session with `SHELLMAN_TRANSPARENCY=on`, `off` stops it. Measured:
  a write of the value already there succeeds and bumps the stamp (1 -> 2); flat -> blur at a
  fresh sign-in is still to be seen. Not handled: the real shell may also turn it off under
  energy saver; which signal it follows is not known yet.
- **Tests**: `SHELLMAN_SESSION_APPS="exe|arg|arg;exe2"` replaces the app list (and turns off
  startup items and the safety net, which `SHELLMAN_SESSION_SAFETY=on` brings back;
  `SHELLMAN_SESSION_FALLBACK=<exe>` stands in for explorer.exe). `examples/child.rs` is a
  throwaway child.

### The shell swap

`shellman-session --install` saves the current per-user value
(`HKCU\Software\shellman\session`) and sets `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon`
`Shell` to the session's full path; it refuses if `shellman-bar.exe` is not beside it. Windows
reads the per-user value before the machine's (`explorer.exe`). It takes effect at the next
sign-in. `--uninstall` puts the saved value back, or deletes the value, and leaves a Shell value
that is not shellman's alone. Both are unit-tested against a fake registry and have never been run.

If the screen is empty after the swap: hold Shift at sign-in (the safety net starts Explorer); or
Ctrl+Alt+Del, Task Manager, Run new task, `explorer.exe` (then `shellman-session --uninstall`
from a terminal); or `reg delete "HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon" /v Shell /f`
from Run new task with `cmd`. Signing in as another user is unaffected (the value is per user).

## Icons

- shellman's own apps are drawn in the user's accent wherever shellman shows them: dock,
  switcher, launcher, settings, window icons.
- The icon embedded in each exe is fixed at the default accent.
- Icon packs (`crates/core/src/icon_pack`): freedesktop themes or folders of images, added in
  Settings › Appearance › Icon pack, matched to Windows apps by exe name, AUMID and shortcut name
  through an alias table. `icons.json` holds the choice and per-app overrides.
- **Islands**, the built-in pack and the default: the mockups' dock tiles, 66 icons (shellman's
  own apps, the Recycle Bin empty and full, ~35 kinds of app, ~23 well-known apps in their own
  colours). Sources in `assets/icon-packs/islands` (written by `tools/islands/make.py`; README
  there), compiled into every app by `crates/core/build.rs` (about 135 KB an exe) and known by
  paths that exist nowhere on disk. A small variant (heavier glyph) is drawn at 64 px and under.
  No `icons.json`, or one without a `pack` key, means Islands; `"pack": null` is "Windows icons"
  and stays so. Shellman's own apps take a pack's icon only by exact name (`shellman-files`), so
  other packs leave them in the accent.

## Order of work

1. Dock hover previews (in progress).
2. Win+1-9 (the lone Win key is done).
3. Dock: drag-and-drop onto icons, badges and progress, Recycle Bin empty / full.
4. Top bar: "now" pill, notifications and calendar.
5. Desktop: widgets (Today, Now playing) and arrange mode; a glass right-click menu.
6. Task View.
7. `session`: built. Next: try the swap on a spare user account, then on the real one.
