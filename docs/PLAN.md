# shellman

A replacement for everything `explorer.exe` does, in one visual style: Windows 11 glass, one
accent color, fast.

## Pieces

| App | Replaces | Status |
| --- | --- | --- |
| `apps/files` | File Explorer windows | working: tabs, list/grid, thumbnails, shell menu, auto-refresh |
| `apps/bar` | taskbar, tray, clock, Start button (as a top bar and a dock) | next |
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

## Bar plan: a top bar and a dock

Rule: **the top bar is the system, the dock is apps.** Both are windows of one process,
`apps/bar`, since they share the window list, hooks and icon cache.

| Explorer job | Where |
| --- | --- |
| Start / launcher | top bar, left corner (Win key too) |
| Running and pinned apps, switching windows | dock |
| Tray icons, all of them | top bar, right (up to 8, the rest behind a chevron) |
| Network, volume, battery | top bar, one status pill |
| Clock, calendar, notifications | top bar, right corner |
| Files, Recycle Bin | dock ends |
| Space for maximized windows | both bars reserve theirs |

**Top bar**: 32 pt, full width, flush to the edge so edge pixels hit items.
- Left: the launcher glyph in the corner, then the active app's name and window title.
- Centre: empty, except a "now" pill for media, copy progress, or an app asking for attention.
- Right: tray, overflow chevron, status pill (scroll for volume), clock ("Wed 11 Nov  14:32").
  The clock wakes once a minute on the boundary, plus on time changes and resume.

**Dock**: bottom centre, a floating glass pill.
- Size: 40 pt icons in 52 pt cells, 56 pt tall. Reserved space; autohide is a setting.
- Corners: Windows' own 8 pt window radius. DWM paints glass over the whole window and ignores
  window regions (measured), so a larger radius would mean giving up real blur.
- Order: Files, pinned apps, divider, running unpinned apps, divider, Recycle Bin.
- Running dots (up to three for several windows); an accent bar marks the app in front;
  attention pulses at most three times.
- Click activates, or minimizes the app in front. Middle click opens a new window; scroll cycles
  windows. Hovering shows live DWM previews, and sliding along the dock scrubs them.
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
- Fullscreen apps and games: when the foreground window covers its monitor, the bars on that
  monitor step aside.
- Space: next to Explorer through `SHAppBarMessage`; as the shell, by setting the work area per
  monitor directly.
- Monitors: a top bar on every monitor; the tray and dock on the primary at first.
- Window list from `RegisterShellHookWindow` plus narrow WinEvent ranges, never polling.
- Tray: copy each icon (owners destroy theirs), coalesce animated updates, check that clicks reach
  elevated apps.

**Order of work**
1. ~~Prototype the dock's rounded glass~~ done: system 8 pt corners, real blur.
2. Dock: running apps and clicks done; next pinned apps, previews, reserved space.
3. Top bar: launcher button, active app, clock, status.
4. With Explorer stopped: tray, work area, Win keys.
