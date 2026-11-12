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
| `apps/desktop` | wallpaper window, desktop icons | working: wallpaper per monitor; keeps Windows' modern-app host running. Next: desktop icons, right-click menu |
| `apps/settings` | Settings pages for the shell | working: Appearance, Dock, Top bar, Launcher, About |
| `apps/session` | the shell process itself | last |

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
| Tray icons, all of them | primary's top bar, right (up to 8, the rest behind a chevron) |
| Network, volume, battery | top bar, one status pill |
| Clock, calendar, notifications | top bar, right corner |
| Files, Recycle Bin | dock: Files after the launcher, the bin at the end |
| Space for maximized windows | each monitor reserves its top bar and dock band |

**Top bar**: 32 pt, full width, flush to the edge so edge pixels hit items.
- Left: the launcher glyph in the corner, then the active app's name and window title.
- Centre: empty, except a "now" pill for media, copy progress, or an app asking for attention.
- Right: tray, overflow chevron, status pill (scroll for volume), clock ("Wed 11 Nov  14:32").
  The clock wakes once a minute on the boundary, plus on time changes and resume.
- Tray and its chevron appear on the primary monitor's top bar only.

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
  `bar.json`). The tray stays on the primary's top bar.
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
- Apps come from the shell's Apps folder and launch via `shell:AppsFolder\<id>`.

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

## Settings

- One app writes the shared files under `%APPDATA%\shellman`. Every app follows them through its
  folder watch.
- No OK / Apply: a change applies as it is made.
- Opened from the top bar glyph's right-click, the dock Launcher icon's menu, and the launcher's
  "shellman Settings" row.
- Single instance.

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

The supervisor starts it when nobody owns the shell window and no Explorer runs, waits on its
process, and when Winlogon replaces it with Explorer stops that one Explorer (started after the
host died, by userinit), re-announces the tray (`TaskbarCreated`) and starts the host again; three
failures in ten minutes and it gives up. A clean stop is left alone. shellman never claims the
shell window itself. On shellman's exit the host is left running: harmless, and ending it any
other way than WM_QUIT would bring Explorer back. "Keep Settings, Calculator and the Store
working" (About page, `shell.json`, default on) turns it off; `SHELLMAN_APP_HOST=off|on` overrides
for tests. Its log is `%LOCALAPPDATA%\shellman\app-host.log`.

The bar puts its work areas back when they change under it: an Explorer that runs even briefly
sets every monitor's for its own taskbar.

If the exe is missing or does not take the shell window, nothing else changes: modern apps stay
unavailable and the Control Center hides its Settings links, except where a Control Panel applet
opens without Explorer (Bluetooth: `bthprops.cpl`; Sound: `mmsys.cpl`; also `main.cpl`,
`sysdm.cpl`). Never `ncpa.cpl`, `control.exe` or Power Options (they start Explorer) or
`desk.cpl` (it opens Settings).

Not yet tried with the host running: toasts, the touch keyboard, snap layouts, sleep and resume,
display changes, long uptime.

## Icons

- shellman's own apps are drawn in the user's accent wherever shellman shows them: dock,
  switcher, launcher, settings, window icons.
- The icon embedded in each exe is fixed at the default accent.

## Order of work

1. Dock hover previews (in progress).
2. Win+1-9 (the lone Win key is done).
3. Dock: drag-and-drop onto icons, badges and progress, Recycle Bin empty / full.
4. Top bar: "now" pill, notifications and calendar.
5. Desktop icons and menu.
6. Task View.
7. `session`: start and restart the apps, and the shell swap.
