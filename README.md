# shellman

A replacement desktop shell for Windows 11, written in Rust. It takes the place of
`explorer.exe` as the shell: its own top bar, dock, desktop, launcher, window switcher, file
manager, terminal, settings and task manager, all drawn as floating islands of blurred glass.

It is a personal project and a work in progress. The plan, the state of every piece and the
known gaps are in [docs/PLAN.md](docs/PLAN.md); the design mockups are in
[docs/design](docs/design).

## Screenshots

### Files

Tabs, a labelled sidebar that folds to icons, tags, and glass context menus.

![Files with an item's context menu](docs/screenshots/files.png)

### Terminal

Tabs, profiles, find, selection and mouse support, on a bundled ConPTY.

![Terminal with the find bar open](docs/screenshots/terminal.png)

### Dock

Live window previews on hover, drag to reorder, drop files onto icons, badges, progress bars
and the Recycle Bin.

![Dock with window previews](docs/screenshots/dock-previews.png)

![Dock with progress bars and an attention mark](docs/screenshots/dock-badges.png)

### Top bar

A "now" pill for the next event or what is playing, the calendar under the clock, and
notifications under the bell.

![The now pill beside the clock](docs/screenshots/now-pill.png)

| Calendar | Notifications |
|---|---|
| ![Calendar panel](docs/screenshots/calendar.png) | ![Notifications panel](docs/screenshots/notifications.png) |

### Desktop widgets

Today (calendar and weather) and Now playing, beside the tray.

![The Today widget under the tray](docs/screenshots/widgets.png)

### Settings

![Settings, Terminal page](docs/screenshots/settings.png)

## What is in the repository

| Folder | What it is |
|---|---|
| `apps/session` | The shell process Windows starts at sign-in; starts and supervises the rest |
| `apps/bar` | Top bar, tray, dock, notifications, calendar |
| `apps/desktop`, `apps/shelf` | Wallpaper, the file shelf and the widgets |
| `apps/launcher`, `apps/switcher` | App launcher and Alt+Tab |
| `apps/files`, `apps/pickhook` | File manager, and its hook for other apps' Open and Save dialogs |
| `apps/terminal` | Terminal |
| `apps/settings`, `apps/control`, `apps/tasks` | Settings, quick controls, task manager |
| `apps/setup` | Install, update, status and uninstall |
| `crates/core`, `crates/ui` | Shared system code, and the glass, rail and menu components |

## Build and install

Windows 11 and a current Rust toolchain.

```
cargo build --release --workspace
target\release\shellman-setup.exe install --from target\release
```

That installs the apps without changing the Windows shell. Making shellman the shell is a
separate, deliberate step, and holding Shift at sign-in still starts Explorer:

```
%LOCALAPPDATA%\Programs\shellman\shellman-session.exe --install
```

To go back to Explorer and remove it:

```
%LOCALAPPDATA%\Programs\shellman\shellman-terminal.exe --unregister-default-terminal
%LOCALAPPDATA%\Programs\shellman\shellman-session.exe --uninstall
target\release\shellman-setup.exe uninstall
```

`shellman-setup uninstall` does not yet remove the file dialog hook's registry entries or the
"shellman font check" sign-in entry; see the known gaps in the plan.

## Known problems

- With shellman as the shell, Windows does not start packaged apps' background tasks at
  sign-in (found with Dolby Access OEM, which stopped running). Not fixed.
- A dock pin for an app whose window belongs to a helper process (Steam) shows the wrong icon
  or does not start the app after a restart. Unfinished work is on the `wip/dock-pins` branch.
- Opening shellman's own apps and panels is slow after a few idle minutes. Unfinished work is
  on the `wip/open-speed` branch.
