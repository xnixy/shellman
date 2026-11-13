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
| `apps/shelf` | desktop icons, the desktop's right-click menu | stage 1 working: the shelf on every monitor or the primary only (drop zone, Desktop / Downloads / Recent stacks that fan open, drag out, drop in), the right-click menu on every monitor, Settings › Desktop; stage 2 working: the Today and Now playing widgets top right (primary or every monitor), Edit desktop…. Next: arrange mode |
| `apps/tasks` | Task Manager (Ctrl+Shift+Esc, through the switcher's hook) | working: Processes (apps and background, grouped, sort, search, End task / End process tree / details), Performance (CPU, memory, GPUs, disks, network), Services (read-only: status, startup type, PID, description, sort, search, go to process; read through the SCM in `crates/core/src/services`, off the UI thread, only while the page shows). End task / tree try unelevated first and, only when Windows refuses, go through `shellman-elevate`'s `end-processes <pid>:<start time>...` behind the UAC prompt (the helper rechecks start time and refuses critical processes); critical processes stay un-endable. Reads through `crates/core/src/procs` |
| `apps/terminal` | Windows Terminal | stage 2: tabs, selection, mouse, profiles; `-d`/`-p`/`-e`/`--hold`; shellman's "Open Terminal here" opens it; can be Windows' default terminal (`--register-default-terminal`). Next: Settings controls, find, splits |
| `apps/settings` | Settings pages for the shell | working: Appearance, Desktop, Dock, Top bar, Launcher, About; rebuilt Windows pages: Sound, Display, Apps, Startup, Fonts, Mouse & keyboard, Power, Date, time & region, Storage, Default apps (changes through Windows Settings), Windows Update |
| `apps/session` | the shell process itself | the shell: the per-user Winlogon `Shell` value points at it (in daily use since 2026-11-12). Starts and supervises the apps, runs startup items, safety net, `--install`/`--uninstall` |
| `apps/setup` | the installer | `shellman-setup`: `install`, `update [--restart]`, `status`, `uninstall`; a console tool that also works from a release folder (see Install and update) |

Shared code:

- `crates/ui`: theme (palette from one accent), glass window frame, widgets.
- `crates/core`: shell plumbing that has no layout: file ops, shell images, the Windows context
  menu, folder watching, the app index (the shell's Apps folder), live DWM previews
  (`live_thumb`), shared settings types (`settings::{dock,bar,launcher}`), and settings files
  under `%APPDATA%\shellman`. `SHELLMAN_CONFIG_DIR` overrides that folder, for tests.
- `crates/buildinfo`: the build stamp (`<crate version>+<git describe> <commit date>`, or
  `SHELLMAN_BUILD_STAMP`) every exe carries as its `ProductVersion`, put there by each app's
  `build.rs` (`shellman_buildinfo::apply`).
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
| Clock, calendar, notifications | top bar, right corner (the list opens under the clicked bar's clock, on that monitor, at its scale and work area) |
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
- Right-click on an icon: shellman's glass popup menu (see the desktop's right-click menu) with
  Pin / Unpin and Close. On the Launcher
  icon: Settings. On the Recycle Bin: Open (in Files, never Explorer) and Empty Recycle Bin
  (grey when empty; a native confirmation first). The dock window is only as tall as its icons, so the menu cannot live in it.
- Drag to rearrange: press an icon and move past 6 pt to pick it up; it rides the pointer along the
  dock, the others slide apart (the dock's own window, no extra window), release drops it, Esc or
  releasing off the dock cancels. Pinned apps reorder among the pinned (saved to `dock.json` at
  once; every dock follows); a running unpinned app dropped among the pinned is pinned there; running
  apps are not reorderable among themselves, and the Launcher, Files, divider and bin never move.
  Unpinning stays in the right-click menu.
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
  on the left edge and two widgets top right; "Clean" (wallpaper only) is the other style.
  Settings in `desktop.json` (`settings::desktop`): style, shelf on/off, edge, stacks, where
  dropped files go, and the widgets (on/off each, which displays, the calendar account without its
  password, the weather's place and units).

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

- On every monitor or the primary only: Settings › Desktop › "Show shelf on" (`shelf.on` in
  `desktop.json`: `Primary` or `All`, default `All`). Every shelf shows the same stacks from the
  same folder watches and thumbnails (one process); one fan is open at a time, on the shelf it
  was opened from. Each is placed in its own monitor's pixels, on the edge setting's side of its
  work area, centred between its bars.
- Windows: the root viewport carries the primary's shelf (and hears the desktop's menu request);
  every other monitor gets an immediate viewport of its own, keyed by device name
  (`shellman shelf <device>`, never the root's title). Monitors come from
  `core::monitors::all` (the bar still has its own copy of that enumeration). On a display,
  work-area or DPI change the list is read again (`displays::targets`, `reconcile`): a monitor
  still there keeps its window and is only placed again; a monitor gone (or a setting that no
  longer gives it a shelf) has its glass dropped, drop target revoked and subclass removed, and
  its viewport is no longer declared, so the window closes. A monitor reporting a zero, tiny or
  odd rectangle, a work area outside it, or a scale that is not one is skipped until it reports
  something sane: no window is made or sized for it.
- Sizes (`displays::place`): a window is as wide as the shelf plus its fan (660 pt) and as tall
  as the shelf plus the furthest any fan of any stack can open above and below it, plus the
  shadow, cut to the monitor; never the monitor's size (990x1248 px at 150%, 1980x1915 at 300%
  on 3840x2160). The size changes only with the monitor, the edge or the number of stacks, never
  per fan, and must pass `glass::guard::window` unchanged; a monitor too narrow for it (under
  ~660 pt wide) gets no shelf. Its glass is attached once when the window is first found. Only
  its islands show and take clicks (`set_islands_only`).
- Desktop level: a tool window that never activates, so never in Alt+Tab or the dock. Whenever
  anyone moves it in the z-order (showing it, the window library), the move is redirected to just
  above the topmost visible desktop window (`ShellmanDesktop`, `Progman`), found walking from the
  top: other apps keep windows under the wallpaper (an "Open With" helper was found there), so the
  first non-desktop window from the bottom is the wrong place. Other windows coming forward never
  move it, so this costs nothing at rest. While a stack is open the window is topmost and has the
  focus (for Esc); it goes back down when the fan has closed.
- Steps aside for fullscreen apps like the bars, per monitor: the window watcher posts a check
  to the root window, which asks `foreground_covers_monitor_of` for every shelf window; a
  shelf shows no islands while its monitor is covered.
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
The shelf shows shellman's glass popup menu there, the component the dock's menu uses: New (Folder,
Text document, Markdown note), Paste (greyed with nothing to paste), Open in Files, Open Terminal
here, Edit desktop…, Change wallpaper…, Desktop settings, Display settings. "Edit desktop…" opens
Settings on the Desktop page (`--page desktop`, as "Desktop settings" does); the design's arrange
mode (dragging the shelf and widgets about) is not built: the widgets are a fixed stack top right.
It works on every monitor: shellman's wallpaper window covers each monitor (one per monitor in
the desktop process), each hands its right-click over with the point, and the popup takes the
work area and scale of the monitor at that point (checked at 150% left of the primary and 300%
below right). A shelf started at 18:39 on 12 Nov from the stage-1 build kept running after the
install was replaced, and showed an older menu (white, no icons, no glass) on the 300% monitor; the
running processes must be restarted after an install.

No shortcut hints: none of these has a shortcut on the desktop yet.

A first version drew shellman's own menu on glass in an egui window as large as the clicked
monitor. Opening it repeatedly crashed `dwm.exe` (12 Nov, 18:00:02, `dwmcore.dll`, E_INVALIDARG:
DWM fails fast when a composition surface gets a size of 0, negative or above 16384), from a glass
panel created and destroyed per menu on a monitor-sized window (and, in one state, re-attached
every frame); a monitor-sized topmost window also made the bar hide as for a fullscreen app. Rule
from it: never a window as large as a monitor for a popup, never create glass per click, never a
size that has not been checked.

### The widgets (stage 2)

Today and Now playing, as the design's "Shelf + Widgets": a fixed stack top right of the work
area (372 pt wide, 40 pt from the right, 16 pt under the top bar, 16 pt apart, radius 22), real
glass islands following the glass slider, dark and light.

- Where: in the shelf process (`apps/shelf/src/widgets`), not an app of their own: it already has
  desktop-level glass windows that never activate, are never in Alt+Tab, step aside for a
  fullscreen app per monitor, and follow `desktop.json` live. Each monitor with widgets gets one
  more immediate viewport (`shellman shelf widgets <device>`), with the shelf windows' manners
  (`window::prepare_other`), its glass attached once. Size (`widgets::place`): the widgets'
  column plus the shadow, as tall as the tallest stack (Today with three rows and a heading, then
  Now playing), cut to the work area: 452x569 pt (678x854 px at 150%), never the monitor (and at most half its width),
  through `glass::guard::window` unchanged; worked out per monitor and scale, never per frame.
  Only the islands show and take clicks; an island changes size only when its content does.
- On: Settings › Desktop › "Show widgets on" (`widgets.on`: `Primary` default, or `All`).
  Off in Clean, with "Show widgets" off, or with both widgets off.
- Idle: no repaint loop. Frames come from data arriving (the feeds worker, the media session),
  the pointer, a minute tick for the date and the "in 25 min" chips, and a one-second tick only
  while something plays (its time and progress line); none while a fullscreen app covers the
  monitor. Measured 0 ms of CPU in 15 s at rest (debug build, no media session).
- Today: weekday, the day's number large (Segoe UI Light, `theme::light_font`), the month; the
  weather beside it (icon, temperature, "London · light rain", "H 8° · L 3°"); then up to three
  rows of what is left of today (all-day first, then by time; a calendar colour dot; the next one
  heavier with a chip "in 1 h 19 min" or "now"), or tomorrow's under a heading when today is done.
  Never blank: "Calendar not connected", "Sign-in failed", "Connecting to your calendar…", "Can't
  reach the calendar" (cached events still show), "Nothing on today or tomorrow"; the first two
  open Settings › Desktop on a click.
- Calendar (`core::calendar`): CalDAV, built for Fastmail (`https://caldav.fastmail.com/`, any
  server URL works): `/.well-known/caldav` (redirects followed by hand; credentials only ever go
  to the configured host, and over HTTPS unless it is localhost), current-user-principal,
  calendar-home-set, the calendars (Depth 1, VEVENT ones), then a `calendar-query` REPORT per shown
  calendar for a day back to 14 days ahead. iCalendar parsed here (`ical`): folding, parameters,
  escapes, DATE / floating / UTC / TZID times, DURATION; zones from the calendar's VTIMEZONEs, then
  ICU (in Windows) for names it does not define, the viewer's zone from Windows; recurrences
  expanded in the event's wall time (`rrule`: DAILY/WEEKLY/MONTHLY/YEARLY, INTERVAL, COUNT, UNTIL,
  BYDAY with ordinals, BYMONTHDAY, BYMONTH, BYYEARDAY, BYSETPOS, WKST), EXDATE, RDATE, overridden
  and cancelled instances. Every 15 min, on wake (`WM_POWERBROADCAST`) and on a network change
  (`NotifyNetworkConnectivityHintChange`), 30 s then 2 min after a failure. Cached (events only)
  in `%LOCALAPPDATA%\shellman\cache\calendar.json` and shown at once at the start.
- Password: only in Windows Credential Manager (generic credential `shellman/caldav`, UTF-16 as
  Windows' tools write it; `shellman/caldav/<instance>` for a test copy). Never in a json file,
  a log or a debug print. Settings writes it on "Test connection" and bumps `calendar.saved` so
  the shelf tries it at once; "Forget password" deletes it.
- Fastmail: user name = the Fastmail address; an app password from Fastmail › Settings › Privacy
  & Security › App passwords (Manage app passwords and access) › New app password, with access to
  Calendars (CalDAV). Not yet run against Fastmail itself: built and tested on recorded answers
  and a local mock server.
- Weather (`core::weather`): Open-Meteo forecast (current temperature, weather code, is_day;
  today's max/min), no key, generic User-Agent `shellman`, only the coordinates sent. The place
  is typed in Settings and found once through Open-Meteo's geocoding, saved as name + lat/lon; no
  lookup by IP. Units from the Windows region (`iMeasure`) unless set to °C or °F. Every 30 min,
  cached in `cache\weather.json`; a cached reading shows for up to 6 hours without the network.
- HTTP: `core::net`, WinHTTP (system TLS and proxy, no crate, no async runtime), on worker
  threads; redirects are not followed automatically; 15 s timeouts.
- Now playing: `core::system::media` (the Control Center's watcher, now also with the album and
  the timeline): art 54 pt, title, "artist · 1:52 / 4:03", a 4 pt progress line, previous /
  play-pause / next. Hidden when there is no session (the design has no empty state).
- `SHELLMAN_ONLY_MONITOR=<x>,<y>` (tests only) keeps a test copy of the shelf process to the
  monitor holding that point, taken as the primary.
### The glass popup menu (`shellman_ui::menu::popup`)

The one menu of every shellman app: the desktop's (shelf), the dock's (bar), every menu in Files
and the dropdowns in Settings are this component, so they look and behave the same. Files never shows
the Windows context menu itself: it reads the shell's entries (`core::shell_menu`) into the popup and
invokes the picked one.

- API: `popup::declare(ctx)` in every root frame; `PopupMenu::<T>::open(ctx, anchor, entries)`;
  `take_pick()` in a later frame of the viewport that opened it (woken when something is
  picked); `update(entries)` replaces the entries of the open menu in place (rows that arrive
  late; nothing happens when they are equal); `take_opened()` hands back a submenu's `on_open`
  value when it opens (for submenus filled on demand). Anchors, in physical pixels: `at(x, y)`,
  `above(x, y)` (the dock), `in_window(ctx, pos)` for a point of the app's window, `under(ctx,
  rect)` / `over(ctx, rect)` for a dropdown from a button (it flips to the other side of the
  button, never over it).
- Entries: items (glyph or picture icon, `&` letter, hint such as a shortcut, disabled, checked,
  danger in red), separators, headings, quiet notes ("Loading…"), pictures someone else drew
  (owner-drawn shell entries), and submenus three levels deep (deeper ones show disabled).
- `menu::dropdown::dropdown(ui, id, width, shown, rows)`: a select box whose list is the popup,
  the current row ticked, in place of egui's `ComboBox`.
- One window per process: an egui viewport declared from the first open on, found by its title,
  made a topmost popup tool window, and given its glass panel once, while still hidden. An open
  only sizes and places it (hidden, cloaked), lays out the glass islands, shows it with the focus,
  and uncloaks it after two frames at the target monitor's scale. Closing cloaks it, hides its
  glass pieces (empty region, no clicks) and parks it shown-but-cloaked at its last size: a hidden
  egui window spins on a pending frame, and the window is never shrunk to nothing.
- The window is the bounding box of the menu and its open submenus, each an island; the shadow
  lies in the glass's backdrop window. Every size passes `place::guard` (finite, at least 24 px,
  no larger than the work area, never above 8192) before a window or glass sees it.
- Placed in physical pixels on the clicked monitor's work area, laid out at that monitor's scale;
  flips left/up at the edges; a submenu goes right, else left, and deeper ones keep the side the
  first took. A level taller than the work area scrolls (wheel; the keyboard keeps the lit row
  in view; a thin bar shows where it is). Long hints are cut with an ellipsis.
- Closes on a pick, Esc, a click outside (`core::outside`), the focus leaving, or another menu of
  the process opening; the focus goes back to the window that had it on a pick or Esc. Keys:
  arrows, Home/End (and PageUp/PageDown), Left/Right through the submenus, Enter/Space, `&`
  letters and first letters. A submenu also opens after the pointer rests 200 ms. No animation.
- Measured (12 Nov, test instance, secondary monitors at 150 % and 300 %): 0 ms of CPU in 3 s
  after a menu closed; the top bar and dock stay up while it is open; no `dwm.exe` errors.
  `cargo run -p shellman-ui --example popup_menu -- <x> <y> [--sub] [--light] [--glass 0.2]`
  opens one to look at.

Where it is used (13 Nov):

- Files: the item / multi-selection / background menu: Files' own entries (about a dozen rows),
  then one "More options" submenu holding the shell's entries (icons, owner-drawn rows, deferred
  submenus filled on open; entries that repeat Files' own left out). The shell's menu is read
  when the right button goes down (or when More options opens), no longer ahead on every
  selection change. Then the Recycle Bin's item and background
  menus, a tab's menu, the sidebar's row and background menus, both Sort dropdowns, Move to /
  Copy to, and the picker's file type list. The Menu key and Shift+F10 open the menu beside the
  cursor's row. A dropdown's button clicked again closes its menu. Seen on the left monitor, dark
  and light: item menu, Tags submenu, 7-Zip three levels deep, background menu at the bottom
  right (flips up), Sort, a sidebar row, Move to.
- Settings: the dropdowns of Date & time (first day, date patterns), Display (orientation,
  scale, resolution, refresh rate), Sound (spatial, format), Power (plan, timeouts) and Fonts
  (face per weight).
- Still egui popups: Settings' time zone and font family lists (they have a search field),
  Settings › Desktop's drop folder list, Files' tag editor in the details island (its names turn
  into fields), and the terminal's profile menu (an in-window island of flat rows).
- No menus of their own: Task Manager, the launcher, the switcher, the top bar (right-click on
  the Start mark opens Settings), notifications. The Control Center's power choices are an
  inline two-click confirm, not a menu. Tray icons' menus are their apps' own.
- Later: the shell's verbs could also sit in a "More options" submenu (they are read the same
  way); owner-drawn third-party rows stay pictures rendered on an opaque background.
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
- Sidebars: Settings, Files and Task Manager draw their left rail with one component
  (`shellman_ui::rail`): a blurred-glass island the window's whole height (level with the top
  pill, so no empty corner above it), icon and name per row, group headings (a thin
  line when folded), a fold button at its foot, names as glass pills on hover when folded. Below
  800 px the window shows icons only, whatever the choice. Rule: Appearance › Sidebars ("Show
  names" / "Icons only", `appearance.json` `rail_collapsed`) is the default for every app; an
  app's fold button changes that app only (`"rails": {"files": true}`, an app left out follows
  the default); choosing in Appearance sets the default and drops every app's own choice. Rows
  are data; Files attaches its drops, middle clicks and menus to the rows' responses.
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

`apps/terminal` (`shellman-terminal.exe`) is to replace Windows Terminal. Order of work: stage 1
(one shell, GPU grid) and stage 2 (tabs, selection, mouse, profiles) are done, shellman's own
entry points open it, and it can be Windows' default terminal (below).

- **Command line** (`cli.rs`, tested): `-d/--dir <folder>` (the first tab starts there; a WSL
  profile gets it as `--cd`), `-p/--profile <name>`, `-e <command line>` or `-- <command line>`
  (run that instead of the shell; the rest of the line reaches CreateProcess exactly as typed,
  taken by offset from the raw line), `--hold` (the tab stays after the program exits, saying
  its exit code). A bare command line (`shellman-terminal cmd /k ver`) still works. Bad
  arguments print usage to the parent console and exit 2.
- **A second start**: `new_instance` in `terminal.json`, `"window"` (default, as Windows
  Terminal) or `"tab"`: the launch (folder, profile, command, hold; nothing else) goes as JSON
  over `\\.\pipe\shellman-terminal-<session>` to the first window, which re-checks it, opens the
  tab and comes forward (`instance.rs`). The pipe refuses remote clients; its default DACL lets
  only the user, SYSTEM and administrators write.
- **Entry points**: `shellman_core::terminal::open_at(dir)` is the one way shellman opens a
  terminal: the desktop menu's "Open Terminal here" (`apps/shelf`, which had its own
  `wt.exe -d` code) and Files' new "Open Terminal here" (folder background and a folder's menu).
  Launcher, dock and Task Manager start apps by path and have no terminal entry of their own.
  The choice is `default_terminal` in `terminal.json`: `"shellman"` (default), `"windows-terminal"`
  or `"custom"` with `custom_terminal` (a command line, `{dir}` for the folder). A missing choice
  falls back: shellman's terminal (sibling exe), Windows Terminal (app alias `wt.exe -d`, then the
  activation manager), Command Prompt in a new console. Settings › Terminal sets it (below).
- **Settings › Terminal** (stage 3, `apps/settings/src/sections/terminal.rs`; rail entry and
  search words): default profile (the names from `shellman_core::terminal::profiles`, which the
  terminal now builds its profiles from too), font family and size, lines of history,
  copy-on-select, "New terminal opens in" (`new_instance`), "Open Terminal here uses"
  (`default_terminal`, `custom_terminal`), and "Use shellman Terminal for all console programs".
  `terminal.json` is read and written whole through `shellman_core::terminal::file::File`, so
  fields the page does not know are kept (tested). A running terminal follows the file (its
  settings-folder watch): font family and size, history length (`Term::set_options`),
  copy-on-select, default profile and `new_instance` apply live. The console switch shows the
  registry's `DelegationTerminal` (`terminal::handoff::current`: shellman, Windows Terminal,
  Windows decides, or another CLSID) and runs the installed `shellman-terminal.exe`'s own
  `--register-default-terminal` / `--unregister-default-terminal` beside Settings, off the UI
  thread, then reads the state again; it is disabled without Windows Terminal (one line says
  why). Tested: the state from the value and the command it runs; the switch was not flipped.
- **Windows' default terminal** (`defterm/`, `pty.rs`): where a console program shows
  when it starts with no terminal (a `.bat`, Win+R `cmd`, a program spawning `powershell`). Off
  until `shellman-terminal --register-default-terminal`; `--unregister-default-terminal` undoes
  it. Both are per user, no admin, and print the pair before and after.
  - *Mechanism* (microsoft/terminal `DelegationConfig.cpp`, `srvinit.cpp`, `ITerminalHandoff.idl`,
    `winconpty.h`; Windows Terminal 1.24's `AppxManifest.xml`): the inbox conhost reads
    `HKCU\Console\%%Startup` `DelegationConsole` / `DelegationTerminal`, hands the console to the
    delegation console (`IConsoleHandoff`, OpenConsole), which runs it as a pseudo-console and
    calls `ITerminalHandoff3::EstablishPtyHandoff` (`{6F23DA90-…}`, the only version WT 1.24
    registers a proxy for) on the terminal CLSID with `CLSCTX_LOCAL_SERVER`. Settings' picker
    only lists packaged pairs (app extensions `com.microsoft.windows.console.host` /
    `.terminal.host`, both from one package), but the values take any CLSID and OpenConsole
    creates whatever the terminal value names.
  - *Registration*: `HKCU\Software\Classes\CLSID\{39C0509F-90BF-4A6C-A66D-5CF96BDDC7E2}\LocalServer32`
    = this exe; `DelegationConsole` = Windows Terminal's OpenConsole `{2EACA947-…}`,
    `DelegationTerminal` = ours; the previous pair is kept in `HKCU\Software\shellman\Terminal`
    and put back on unregister (only if shellman is still the choice). The plans are pure and
    tested. Needs Windows Terminal installed (its OpenConsole, and its packaged proxy-stub
    `OpenConsoleProxy.dll` for the handoff interfaces, which an unpackaged process uses too);
    register refuses otherwise.
  - *Handoff*: COM starts `shellman-terminal.exe -Embedding`; an MTA thread registers the class
    (`REGCLS_SINGLEUSE`: one console, one window, as WT's default "new window"). The call's
    `[in]` handles are checked (signal is a pipe, server and client are processes) and
    duplicated; we make the input and output pipes and return OpenConsole's ends.
    `pty::Pty` drives them as `alacritty_terminal`'s `Pty` would: output read on a thread and
    posted to the poller, input queued to a writer thread, resize as the 6-byte signal packet
    (`PTY_SIGNAL_RESIZE_WINDOW` = 8, cols, rows), the end when the client (or the console) exits;
    dropping it closes signal, reference, then the console's process, as `ClosePseudoConsole`
    does, so closing the window ends the program. STARTUPINFO's position and minimized state are
    honoured; a hidden console gets no window (in practice conhost does not hand those over). The
    window writes no settings file.
  - *Tested live on 26300* (values switched for ~3 s, restored, CLSID removed): `cmd /c` started
    with a new console showed in a shellman window with its output, `mode con` reported the
    window's grid (120x33, so the resize went through), the window closed when the program ended,
    and closing the window ended the program and its OpenConsole.
  - *Not done*: a Settings control (the two commands for now); consoles without Windows
    Terminal installed. For that, shellman would bundle an OpenConsole of its own: Microsoft's is
    MIT but its CLSID is compiled in (`CConsoleHandoff.h`), so it has to be rebuilt from
    microsoft/terminal with a new CLSID (VS toolset; ~1 MB `OpenConsole.exe`), and its
    `OpenConsoleProxy.dll` (~40 KB) registered as the proxy-stub for the handoff IIDs under HKCU
    `Interface\{…}\ProxyStubClsid32`, which one third-party report (SonicTerm #1655) found broke
    every terminal's handoff while installed, so it must be tested against WT side by side. The
    `OpenConsole.exe` bundled since stage 3 (for throughput, above) does not change this: it is
    Microsoft's build, with Windows Terminal's `CConsoleHandoff` CLSID compiled in, and the
    NuGet package has no `OpenConsoleProxy.dll`. Using it as the delegation console would mean
    registering WT's CLSID per user to it (clashing with WT's own packaged registration while WT
    is installed) plus a proxy-stub for the handoff interfaces, so the rebuild with a CLSID of our
    own (above) is still the clean way. Packaging (an MSIX/sparse package, which needs a signed package) is only needed
    to be listed in Windows Settings' picker, which is unreachable here anyway.

- **ConPTY** (`conpty.rs`, `pty.rs`; stage 3): pseudo-consoles are made through Microsoft's
  redistributable ConPTY, `conpty.dll` + `OpenConsole.exe` from NuGet
  `Microsoft.Windows.Console.ConPTY` 1.25.260930003 (MIT, Microsoft-signed; files, hashes and
  source in `third_party/conpty/README.md`), when both sit beside `shellman-terminal.exe`; else
  through Windows' own API (inbox conhost). `apps/terminal/build.rs` copies the two x64 files next
  to the exe in `target\<profile>`; **the install must put both in the same folder as
  `shellman-terminal.exe`** (and, like the exes, rename them aside rather than overwrite them while
  terminals run: the dll is loaded and each tab runs an OpenConsole). The dll is loaded by full
  path only (alacritty's own ConPTY code also tried `conpty.dll` from PATH, and it leaked the
  console's pipe ends). Created with `PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH`, so OpenConsole measures
  emoji per code point as the emulator does. The tab's pty is ours for both kinds (created and
  handed over): output read on a thread into a 1 MiB buffer, input on a writer thread. The console
  host is released (`ReleasePseudoConsole`) once the program started, so it exits after its last
  program and its output pipe breaks: a program's end is told to the emulator only after that
  and after the parser took every byte (at most 3 s, for a program that left a child on the
  console; a handed-over console, which is not released, waits for output to go quiet). That
  fixed stage 2's lost last line of a program that exits at once.
- **Emulation**: `alacritty_terminal` 0.26: grid, scrollback and VT parser, on a thread of its own
  per tab.
  Its listener answers the pty directly (cursor reports, OSC 4/10/11 colour queries from the
  theme's palette) and wakes the window. The pty's output passes through `osc.rs` on its way:
  OSC 7 / OSC 9;9 (the shell's directory) and DECSET 9 (X10 mouse), which the emulator does not
  keep. That wrapper also fixes a stall in `alacritty_terminal` on Windows: a read pass stops at
  64 KiB and nothing wakes the loop while its pipe still holds data, so a fast writer (`cat` of
  100 MB in WSL) froze it for good (stage 1 too); passes now end at 48 KiB with a "readable"
  posted to the poller.
- **Structure**: `session.rs` is one shell; `tabs.rs` the tab list's rules (pure); `app/` the
  window (`mod` tabs and layout, `input` keys and shortcuts, `pointer` mouse, `strip` tab strip and
  profile menu, `view` snapshot, blink and zoom); `snapshot.rs` turns the visible grid into plain
  cells (palette, inverse, faint, wide characters, combining marks composed by NFC, the
  selection's colour, the hovered link's underline, the cursor); `render/` draws cells and knows
  nothing of the emulator. Pure and tested: `keys.rs`, `mouse.rs` (every tracking mode and
  encoding), `select.rs` (selection to clipboard text, word stops, URLs, against a real `Term`),
  `osc.rs`, `profiles.rs`, `tabs.rs`, `render/boxdraw.rs`, `app/input.rs` shortcuts.
- **Tabs**: Ctrl+Shift+T / + (default profile), Ctrl+Shift+1..9 (n-th profile), the ∨ menu
  (profiles, a glass island); a new tab opens right after the shown one, in its directory when
  the shell reports one (OSC 7 / 9;9; WSL gets it as `--cd`), else home. Ctrl+Shift+W, middle
  click or × closes (the last closes the window; a shell that exits closes its tab); Ctrl+Tab,
  Ctrl+Shift+Tab, Ctrl+Alt+1..9 (9 the last), click; drag to reorder. Title: the program's OSC
  0/2 title (a console exe's path shortened to its name), else the shell's name and folder. A
  tab not shown keeps parsing but wakes the window only for its activity dot, bell, title or
  exit. The strip is `shellman_ui::tab_strip`, lifted from Files' tab bar (same geometry and
  painting; Files still has its own copy and can switch to it).
- **Selection**: drag (cells), double-click (words: stops are blanks, quotes, brackets, `|`,
  `,`, `;`, so paths and URLs are one word), triple-click (logical lines), Shift+click extends,
  Alt+drag a block; auto-scroll past the edges; drawn into the cells (accent over glass, mixed
  into explicit backgrounds), so it is part of the row hashes and costs no extra pass; it rides
  with its text as output scrolls (alacritty's model). Copy: Ctrl+C with a selection (without,
  ^C), Ctrl+Shift+C, Ctrl+Insert, Enter; right-click copies a selection, else pastes (Windows
  Terminal's way); Esc drops it; copy-on-select is `copy_on_select` in `terminal.json`. Copied
  text joins soft-wrapped lines, trims trailing blanks, uses CR LF, and a whole-line copy has no
  final line break. Paste: Ctrl+V, Ctrl+Shift+V, Shift+Insert, bracketed when asked.
- **Find** (stage 3, `app/find.rs`): Ctrl+Shift+F opens a glass pill in the top row between the
  tabs and the window buttons (as Files' search pill sits in its bar; the tabs' pill gives way).
  It searches as you type, literally (`alacritty_terminal`'s regex search, metacharacters
  escaped), case-insensitive unless "Aa" is lit; Enter goes to the next match up (older),
  Shift+Enter down, wrapping once all are counted; "3 of 120" (a "+" while counting, "No
  results"); Esc or × closes. Matches on screen are marked amber, the current one in the accent,
  inside the cells (so they ride the row hashes like the selection). The count walks the whole
  history bottom-up in 2 000-line slices, 4 ms a frame, so 100 000 lines never stall the window
  (tested: 100k lines counted in slices); the screen's matches are searched afresh per snapshot,
  and the count starts over when output came (at most every 0.4 s), keeping to the view.
- **Mouse to programs**: X10 (9), 1000, 1002, 1003 tracking; normal, UTF-8 (1005) and SGR
  (1006) encodings; focus (1004, also on tab switches); wheel as buttons 64/65 when tracking,
  arrows on the alternate screen with 1007, else scrollback. Shift takes the mouse back.
- **Links**: OSC 8 hyperlinks underline on hover; a URL in the text underlines while Ctrl is
  held; Ctrl+click opens either (http, https, file, ftp, mailto only). OSC 52 copies (reads are
  refused).
- **Cursor**: DECSCUSR shapes; blinking at Windows' caret rate, only while focused, stopping
  (shown) after 15 s without input or output, so an idle window draws nothing.
- **Drawing**: a GPU renderer inside egui's frame (an `egui_glow` paint callback). Glyphs by
  DirectWrite (`crossfont`, with font fallback) into a 2048 px greyscale atlas (colour emoji on a page of their own, above); box drawing
  (light, heavy, double, rounded), blocks, shades, quadrants and Powerline triangles / half
  discs are drawn cell-sized by `render/boxdraw.rs` so they join without gaps (dashes and
  diagonals stay the font's). Three instanced draws: background runs, glyphs, then lines and the
  thin cursor; per-row slots and hashes, only changed rows are sent.
- **Colour emoji** (stage 3, `emoji.rs`, `render/color.rs`): a wide character in the emoji
  blocks, or one with VS16 or a keycap mark, is drawn in colour: Direct2D draws it with
  DirectWrite's colour font (Segoe UI Emoji) into a premultiplied bitmap through WIC (software,
  no device, no window), once, onto a 1024 px BGRA colour page of its own, drawn by a fourth
  instanced draw. Cell widths stay `wcwidth`'s (the emulator's, and ConPTY measures the same way,
  see ConPTY above), so the cursor never drifts; a ZWJ sequence (👨‍👩‍👧‍👦, four wide cells) or an
  emoji with a skin tone (👍🏽, two) is shaped by DirectWrite into one glyph in the first two cells
  and the cells it took after them stay blank, as `wcwidth` terminals show it. Cells name a
  cluster by number (never reused, so row hashes stay true). No emoji on screen: no colour page,
  no draw, nothing rasterised. Flags stay text (Windows' emoji font has none). VS15 keeps text.
- **Palette** (stage 3, `palette.rs`): the 16 ANSI colours come in two sets. Backgrounds, and
  text on a coloured background (or inverse), use Campbell in both themes, so bright white on red
  is white in the light theme too (it was the light set's dark grey, unreadable). Text on the
  glass uses the theme's set, each colour at least 3:1 (WCAG) against its glass, tested: the
  light set as before (3.2 to 17:1), the dark set Campbell with red, blue, magenta and bright
  magenta lifted (they were 2.0 to 2.7:1 on the dark glass, now 3.2 to 3.9). Black on the dark
  glass is left as it is (meant for light backgrounds).
- **Glass and colour**: as stage 1: cells with the default background draw nothing, so the
  island is the shared glass and follows the slider live; the tab strip, window buttons, the
  profile menu and hover labels are islands too.
- **Profiles and settings**: PowerShell 7 (if installed), Windows PowerShell, Command Prompt,
  each WSL distribution (registry `Lxss`, default first, Docker's skipped), Git Bash. The
  terminal's own `terminal.json` in the shellman config folder (`SHELLMAN_CONFIG_DIR` aware,
  written with defaults the first time): `default_profile`, `font_family`, `font_size` (12),
  `scrollback` (10 000), `copy_on_select` (false), `new_instance` and `default_terminal` (above).
  Zoom: Ctrl+wheel, Ctrl+plus/minus, Ctrl+0.
- **Keyboard**: text comes as Windows composed it (dead keys, AltGr, IME commits), so layouts
  are not assumed; AltGr reaches egui without Ctrl and Alt and its characters go as text.
- **win32-input-mode** (stage 3; `keys.rs` `Win32Key`/`Composer`, `app/win32_input.rs`): ConPTY
  asks for it at its start (`CSI ? 9001 h`, seen by `osc.rs`); from then on keys go to the console
  as whole key events, `CSI Vk;Sc;Uc;Kd;Cs;Rc _` (microsoft/terminal spec #4999), so console
  programs get real key-downs and key-ups with scan codes and modifier state (PSReadLine's own
  Shift+arrow selection and Ctrl+C copy, Ctrl+Alt combinations, Shift on its own). egui has no
  key-ups or scan codes, so a subclass of the window procedure reads `WM_KEYDOWN`/`UP` and the
  `WM_CHAR`s `TranslateMessage` already queued for them (a dead key sends nothing, the next key
  carries the composed character; a key that types nothing goes bare; key-ups carry their
  character by `ToUnicodeEx` without touching the dead-key state) and keeps those messages from
  egui, except the terminal's own keys (`input::keeps`: its shortcuts, paste, Ctrl+C / Enter /
  Escape only while it has a selection, Shift+PgUp/PgDn, Alt+F4 / Alt+Space), modifiers (sent and
  also left to egui) and IME keys (committed text goes as text). The encoding is tested against
  the spec's examples; live, a `ReadKey` loop saw Shift's own key-down and key-up (which the VT
  path never sends).

Measured (release, a 1800x1100 px window on a 4K monitor at 150 %, 240 Hz, same placement for
each; `SHELLMAN_TERMINAL_STATS=<file>` logs frame times):

- `cmd /c dir /s C:\Windows\System32`, median: stage 2 1.63 s (7 runs), stage 1 1.62 s (7),
  Windows Terminal 1.53 s (5). (Stage 1's 3.8 s was a maximized window.)
- `cat` of a 100 MB, 3 M-line file in WSL: stage 2 8.9 s (3 runs), Windows Terminal 2.4 s; stage 1
  never finished (the stall above). During it the system ConPTY's conhost uses a whole core and
  the terminal 1.2 (parser thread and UI); frames take 0.25 ms of CPU at 241 fps. So the gap is
  ConPTY: Windows Terminal brings its own (OpenConsole), which passes output through.
- Stage 3, the bundled ConPTY (release, a 1758x1095 px window on the left 4K monitor at 150 %,
  medians of 3, measured while a game ran on the primary monitor, so absolute times are noisy;
  the ratio is the point; final runs interleaved, 3 each): `cat` of the 100 MB file in WSL,
  stage 2 (inbox conhost) 9.6 s, stage 3 through Windows' ConPTY (the fallback) 7.95 s, stage 3
  with the bundled OpenConsole 2.4 s (Windows Terminal was 2.4 s in stage 2);
  `dir /s C:\Windows\System32` 1.62 s, 1.60 s, 1.53 s.
- Idle 20 s: 0.000 CPU-s focused, 0.016 unfocused. A blinking cursor: 2 frames a second for
  15 s, then none. A tab flooding output in the background: no frames.
- Not yet: splits,
  OSC 10/11 answers through the system ConPTY (it does not forward the queries; the answering is
  in place).

## Session

`apps/session` (`shellman-session.exe`) is the one process Windows starts as the shell. Plain Win32,
no egui; it must be the most robust piece (`shellman-core` only for the registry and startup-item
code Settings shares). Winlogon starts it as the user's shell (`--install`); by hand it runs
beside Explorer for tests.

- **Starts** next to its own exe, in order: desktop, bar, shelf, switcher, launcher `--background`,
  control `--background` (250 ms apart). Every child gets `SHELLMAN_SESSION=1`. The bar then leaves
  the residents to the session; started by hand it still starts them itself.
- **Supervises** with one `MsgWaitForMultipleObjectsEx` on all process handles plus the window's
  messages (no polling). Exit code 0 means ended on purpose (the apps have no quit UI yet, and a
  `--background` start that finds its resident exits 0): not restarted, but another running copy
  is adopted if there is one. Any other code is a crash: restart after 0.5 s, 2 s, then 8 s; five
  crashes within a minute and the app is given up on. All decisions are in `policy.rs`, pure and
  tested. Log: `%LOCALAPPDATA%\shellman\session.log` (64 KB, rotated).
- **Restart requests**: `WM_COPYDATA` (`dwData` `SHMR`, text `restart <app>`) to the session's
  window (class `ShellmanSession`, title `shellman session[ <instance>]`; `restart.rs`), sent by
  `shellman-setup update --restart`. The window answers at once: 1 queued, 0 not an app it
  supervises (on demand: Files, Settings, Tasks, Terminal), 2 ending or parked. The supervisor
  posts WM_CLOSE to the app's windows and records that its exit was asked for; when the process
  ends (any exit code) it starts again at once through `Crashes::on_requested_exit`: nothing is
  added to the crash list, no 0.5/2/8 s pause, no give-up. For the bar the slot goes running ->
  start scheduled, never "lost", and the 20 s `Shell_TrayWnd` deadline counts from the new copy's
  own start, so the safety net cannot fire because of a requested restart; a new copy that then
  crashes by itself is an ordinary crash (a broken update still ends in Explorer). An app that has
  not closed after 6 s is ended (still a requested restart). Nothing restarts while the session is
  ending or parked.
- **Single instance**: mutex `Local\shellman-session[-<SHELLMAN_INSTANCE>]`.
- **Adoption**: at start, an app whose exe already runs in this logon session is held, not
  started again (a session that crashed and was restarted by Winlogon, or a bar started by hand).
  An image renamed aside by an update while it ran (`shellman-bar.oldHHMMSS.exe`, which is the
  path Windows then reports) still counts as that app. At start the session also deletes
  `shellman-*.old<digits>.exe` files beside itself that are not in use.
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
  `Shell_TrayWnd` 20 s after its own latest (re)start, the session falls back to Explorer. A bar
  whose restart is pending is never "lost" (a single crash after 80 minutes used to hand the
  session to Explorer, because the 20 s were counted from the session's start). Shift also
  skips every shellman app.
  - The fleet is *parked*: nothing is started or restarted, apps that run are left running,
    restart requests are answered 2. Parking is not ending: the startup items carry on (only a
    real logoff or the window closing stops them; a park used to end them, and the Explorer
    started as fallback skipped them too because the session had already claimed the run).
  - The hand-over (`fallback.rs`, on a thread of its own so the window keeps answering) makes
    Explorer the shell, which it becomes only when nobody owns the shell window; otherwise it
    opens a folder window and nothing else. So, up to two attempts: (1) Explorer already the
    shell (`app_host::explorer_is_shell`: Explorer owns `Shell_TrayWnd` or the shell window; an
    `explorer.exe` folder window does not count) -> nothing to do. (2) The modern-app host
    (ShellAppRuntime, kept by the desktop) owns the shell window -> WM_QUIT to it
    (`app_host::quit_shell`, its clean stop: Winlogon does not start Explorer for it and the
    desktop's host supervisor leaves it stopped), wait up to 12 s for it to exit; another owner
    is logged loudly. (3) Start `explorer.exe` (without `SHELLMAN_SESSION`). (4) Wait up to 15 s
    for a `Shell_TrayWnd` owned by explorer.exe. Not there: the second attempt does (1) to (4)
    again; still not: logged as an ERROR.
  - After it: no Explorer taskbar after both attempts -> supervising resumes once per session,
    with a fresh crash budget (never after Shift); anything else (taskbar, Explorer already the
    shell, or a second failure) -> parked under the watchdog.
  - The watchdog: while parked, every 60 s it looks for any `Shell_TrayWnd` (the bar's or
    Explorer's). None, and still none 10 s later -> the hand-over runs again. At most 5 times an
    hour; when they are spent it logs an ERROR with the manual way out (Ctrl+Alt+Del, Task
    Manager, Run new task, `explorer.exe`) and tries again when the hour has passed. So a parked
    session never stays without a taskbar for more than about 70 s plus one hand-over, unless
    Explorer itself cannot start.
  - The session stays alive throughout, so Winlogon does not restart it in a loop.
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
  `SHELLMAN_SESSION_FALLBACK=<exe>|<arg>...` stands in for explorer.exe: it "has a taskbar"
  while it runs, and a test run never looks at or touches the real shell window, host or
  Explorer; without it the hand-over only logs). `examples/child.rs` is a throwaway child
  (`wait` plays an Explorer that works, `exit 0` one that gives no taskbar). The fleet
  (`supervisor::Os`) and the hand-over (`fallback::Shell`) have fakes for unit tests.

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

## Install and update

`apps/setup` is `shellman-setup.exe`, a console tool with no UI. Everything it touches can be
redirected for tests: `--prefix` / `SHELLMAN_SETUP_PREFIX` (install folder, default
`%LOCALAPPDATA%\Programs\shellman`), `--start-menu` / `SHELLMAN_SETUP_START_MENU`
(`%APPDATA%\Microsoft\Windows\Start Menu\Programs\shellman`), `SHELLMAN_SETUP_CONFIG` /
`SHELLMAN_SETUP_DATA` (`%APPDATA%\shellman`, `%LOCALAPPDATA%\shellman`, only for
`--purge-config`), `SHELLMAN_INSTANCE` (which session to talk to; a test session has its own),
`SHELLMAN_SETUP_SHELL` / `SHELLMAN_SETUP_TERMINAL` (stand-ins for the Winlogon `Shell` and
`DelegationTerminal` values, `none` = absent). `--dry-run` on install, update and uninstall prints
what would happen and changes nothing.

**What is installed** (`manifest.rs`): every `shellman-*.exe` in the source folder (setup itself
included), `shellman_pickhook.dll` (copied, never registered), licence files (`LICENSE*`,
`NOTICE*`, `THIRD-PARTY*`), everything below an `extras\` folder there (or `--extras <dir>`),
keeping sub-paths, and the lines of an `install-manifest` file (`[?] source [-> destination]`;
the one in the folder, else the first in its parents, so the repo's serves `targetelease`).
The terminal's `conpty.dll` and `OpenConsole.exe` go in as lines there once they exist in the repo.
Icon packs are compiled into the exes; there are no font files.

**Safety**: destinations are checked relative names (`paths.rs`: no `..`, drive, device name;
re-checked with link resolution before every write, rename or delete). A file is copied to
`<name>.new<pid>.tmp`, hash-verified, then the installed one is renamed aside to
`<name>.old<HHmmss>.ext` (works for a running exe) and the new one renamed in; if that fails the
aside is renamed back. A named mutex per install folder stops two setups interleaving. The
`shellman-install.txt` record in the install folder lists what setup put there (uninstall removes
exactly that, and its asides).

**Updating a running app**: the file differs by SHA-256; a running exe goes aside and the report
says "replaced (running: takes effect when the app restarts)". Without `--restart` nothing is
closed or restarted. With `--restart` setup asks the session (see Session, Restart requests)
to restart each replaced app that runs; the session itself is only replaced on disk ("takes effect
at next sign-in"); apps the session does not supervise are never closed: setup lists their open
windows as still on the old build. The session clears leftovers at its start; setup also removes
the ones whose process has ended.

**Not done by setup**: making shellman the Windows shell (`shellman-session --install`, an explicit
separate step that setup only tells you about), registering the terminal as the default
(`shellman-terminal --register-default-terminal`), registering the picker hook (`register.ps1`).
`uninstall` refuses while shellman is the Winlogon shell or the default terminal, and while a
process runs from the install folder.

(a) Installing from a release folder (unzipped anywhere):

    .\shellman-setup.exe install                 :: or  install --dry-run  first
    .\shellman-setup.exe status
    :: then, when you want it as your shell:
    %LOCALAPPDATA%\Programs\shellman\shellman-session.exe --install   :: next sign-in; undo: --uninstall

Updating later: unzip the new release anywhere and run `.\shellman-setup.exe update --restart`
from it (`--from <folder>` otherwise).

(b) The developer loop:

    cargo build --release
    targetelease\shellman-setup.exe update --restart

(`status` shows each installed file against `targetelease`, which processes still run from old
images, and the shell and default terminal state.)

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
5. Desktop: arrange mode (drag the shelf and widgets about); widgets done (Today, Now playing).
6. Task View.
7. `session`: done, and it is the real shell (the swap is made; `--uninstall` puts the previous
   value back).
