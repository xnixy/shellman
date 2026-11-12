# shellman

A replacement for everything `explorer.exe` does, in one visual style: Windows 11 glass, one
accent color, fast.

## Pieces

| App | Replaces | Status |
| --- | --- | --- |
| `apps/files` | File Explorer windows | working: tabs, list/grid, thumbnails, shell menu, auto-refresh |
| `apps/taskbar` | taskbar, tray, clock | next |
| `apps/launcher` | Start menu and search | planned |
| `apps/switcher` | Alt+Tab, Task View | planned |
| `apps/desktop` | wallpaper window, desktop icons | planned |
| `apps/session` | the shell process itself | last |

Shared code:

- `crates/ui`: theme (palette from one accent), glass window frame, widgets.
- `crates/core`: shell plumbing that has no layout: file ops, shell images, the Windows context
  menu, folder watching, settings files under `%APPDATA%\shellman`.

Each app is its own process, so a crash in one never takes down the taskbar. `session` starts
and restarts the others.

## Ground rules

- Fast first: nothing O(n) per frame, no blocking I/O on the UI thread, virtualize long lists.
- Small, readable files: one concern each, a few hundred lines at most.
- Everything is built and tested **next to** the real Explorer. Only the last step, `session`,
  swaps the shell, and that swap is a single registry value that can be set back.

## Taskbar plan

A bar along the bottom of each monitor, in the same glass and accent as Files.

**Window**: undecorated, always on top, no taskbar button of its own (`WS_EX_TOOLWINDOW`),
glass backdrop. It registers as an *app bar* (`SHAppBarMessage`), so maximized windows stop at
its edge, as they do at Explorer's taskbar.

**Running apps**
- Which windows count: visible, top-level, not cloaked (DWM hides other virtual desktops'
  windows by cloaking), no owner, not a tool window unless it asks to be shown (`WS_EX_APPWINDOW`),
  and with a title.
- Kept current with `SetWinEventHook` (create, destroy, show, hide, name change, foreground) and
  never by polling.
- Grouped by AppUserModelID (`SHGetPropertyStoreForWindow`), falling back to the exe path, so
  every Chrome window sits under one Chrome button.
- Icons come from the shell image of the exe or AUMID, via `core::thumbs`, which are cached.
- Click: activate, or minimize if it is already in front. Middle click: new instance. Right click:
  close or close all, and later jump lists.
- UWP apps live in `ApplicationFrameHost` windows; their real app is the child core window.

**Pinned apps**: a list of shortcuts in `taskbar.json`, merged with running ones as Windows does.

**Tray** (notification area)
- Apps add icons with `Shell_NotifyIcon`, which sends `WM_COPYDATA` (`dwData = 1`) to the window
  of class `Shell_TrayWnd`. The payload is a 32-bit-handle `NOTIFYICONDATA` behind a small
  header. The tray *is* whoever owns that window class, so it only works while Explorer's
  taskbar is not running.
- On start, broadcast the `TaskbarCreated` message so apps re-add their icons.
- Clicks are sent back to each app's callback window with its callback message, using the
  version-4 message format when the app asked for it (`NIM_SETVERSION`).
- During development the tray is tested with Explorer stopped; the rest runs beside Explorer.

**Clock**: time and date, local format; a calendar flyout comes later.

**Later**: live window thumbnails on hover (`DwmRegisterThumbnail`), jump lists, multiple
monitors, a Start button that opens `launcher`.
