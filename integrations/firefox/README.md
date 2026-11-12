# shellman Islands for Firefox

Turns the Firefox window into the shellman "Islands" look with only `userChrome.css`, `userContent.css`
and a few prefs: floating glass pills and rounded islands, the Windows backdrop visible between them,
no window frame. No extension, nothing in the Firefox install directory.

Optionally (`install.ps1 -Scripts`, see [Scripts](#scripts-optional)) a few small chrome scripts add what CSS
cannot: the live shellman look (accent and glass follow `appearance.json` without a restart), more and smaller
islands (navigation / address / actions / window-buttons pills, a separate tab list, New Tab pill and tool
pill), and glass that stays glass while the window is not focused.

Checked on **Firefox 160.0** (Windows 11, release build, default theme, "Nova" UI, sidebar revamp on).

## What you get

- Top, CSS only: one pill with all toolbar items (the URL bar a soft inner pill, accent ring when focused) and
  the menu button, and the window buttons (min / max / close) as a pill of their own, like Files and Settings.
  With the scripts the toolbar becomes separate pills: navigation (back / forward / reload / home), the address
  bar alone (the widest), the other actions with the menu, and the window buttons. Runs follow the toolbar as you
  customise it: each run of navigation buttons, the address bar, and each run of other buttons is a pill; a
  flexible space or space you place splits a pill.
- Tabs, both layouts styled (vertical is the default):
  - Vertical (`sidebar.verticalTabs`): the tab rail is an island. Collapsed it is an icon-rail pill like the
    Settings rail, expanded it is a wider island with titles. Selected tab = accent fill. Pinned tabs, the
    new-tab button and the tool buttons at the bottom share the style. Works with the sidebar on the left
    or the right (see below).
  - Horizontal: a pill of tab chips above the toolbar pill, selected chip = accent fill.
  With the scripts, vertical tabs are split further: pinned tabs (when any), the tab list, a small "New Tab"
  pill and the tool pill (bookmarks, history, synced tabs, passwords, settings) are separate islands, expanded
  or collapsed, sidebar left or right.
- The page is its own island (22 px radius, hairline border, clipped corners), 12 px from everything.
- The find bar is a floating pill at the bottom of the page island; the bookmarks toolbar a slim pill.
  F11 and page/video fullscreen are edge to edge: no gap, no radius.
- Menus, context menus, panels (app menu, downloads, extensions), the URL bar dropdown: rounded dark glass,
  hairline border, accent selection.
- Bookmarks/history sidebar panel, the bookmarks toolbar and the find bar follow the same language.
- Dark and light follow the system (`prefers-color-scheme`).
- `userContent.css` only touches Firefox's own pages (about:home/newtab transparent, accent on a few
  controls, the sidebar panel's focus colour). about:preferences and every website are left alone.

## Install / uninstall

```powershell
# Close Firefox first. The script refuses to run without an explicit target.
.\install.ps1 -DefaultProfile          # the default-release profile from profiles.ini
.\install.ps1 -Profile C:\path\to\profile
.\uninstall.ps1 -DefaultProfile        # or -Profile ...
```

Install backs up an existing `chrome\` folder and `user.js` into `<profile>\shellman-islands-backup\<timestamp>\`,
copies the five CSS files into `<profile>\chrome\`, and appends `user.js` (this folder) between
`// >>> shellman-islands begin` / `end` markers. Running it again just updates the files. It never touches
anything else in the profile and refuses while Firefox holds the profile (`-Force` overrides).
Then restart Firefox (once with `-purgecaches` if the CSS looks stale).

Uninstall deletes exactly the five files, removes the marked block, puts back anything it overwrote (from the
backup) and deletes the backup folder. Firefox copies prefs that came from `user.js` into `prefs.js` when it
exits, and the scripts do not edit `prefs.js`; to fully revert, reset these in `about:config`:
`widget.windows.mica`, `widget.windows.mica.toplevel-backdrop`, `widget.windows.mica.popups`,
`browser.tabs.allow_transparent_browser`, `sidebar.verticalTabs`,
`toolkit.legacyUserProfileCustomizations.stylesheets`.

## Scripts (optional)

What they add, all in `chrome/shellman-scripts/` (installed into `<profile>\chrome\shellman-scripts\`):

- `shellman-live.sys.mjs`: the **live look**. Reads `%APPDATA%\shellman\appearance.json` (accent `"System"`,
  `{"Preset":n}` or `{"Custom":[r,g,b]}`, and `glass` 0..1) and applies it to every Firefox window, popup and
  Firefox page at once, computing `--shellman-accent`, `--shellman-on-accent`, `--shellman-island-fill` and
  `--shellman-island-opacity` the same way shellman's theme does (light and dark values; Firefox's light/dark
  still follows Windows). It re-applies when the file changes, and for `"System"` when Windows' colours change.
  No file: the CSS defaults stay. The path can be changed with the pref `shellman.appearance.path`.
  *Watching costs:* one worker thread asleep in `WaitForMultipleObjects` on a folder change notification
  (`FindFirstChangeNotificationW` on the folder holding the file). It wakes only when that folder changes (then
  reads one small file, debounced 150 ms) and once every 10 s as a safety tick that does no I/O (so the thread
  can always be stopped). If the folder does not exist or the notification fails, it falls back to a 3 s timer
  that only stats the file. Firefox exits normally (the thread is signalled to stop at quit).
- `shellman-pills.sys.mjs`: labels toolbar items so the CSS can paint separate pills, and adopts a tiny style
  sheet into the sidebar's shadow DOM for the tool pill. It moves nothing, so Customize, overflow and toolbar
  menus keep working; labels are recomputed when the toolbar changes. In Customize mode the CSS-only layout shows.
- `shellman-window.sys.mjs`: keeps the glass when the window is **not focused**. Windows draws the system
  backdrop (Acrylic, Mica and Mica Alt alike) as a flat opaque colour behind an inactive window; that is DWM's
  doing, not Firefox's (every chrome background is already transparent, so CSS cannot change it). When the window
  loses focus the script sends it `WM_NCACTIVATE(TRUE)` once, which keeps DWM's backdrop live. Focus, keyboard and
  Firefox's own activation handling are unchanged.
- `shellman-glass.sys.mjs`: real see-through gaps with shellman's own glass, only when `shellman_glass.dll` is
  installed too (`-GlassDll`); see [Real gaps](#real-gaps-optional-a-dll). Without the DLL it does nothing.
- `shellman-watch.worker.js`: the watcher thread used by the live look.

How they are loaded: Firefox's *autoconfig* runs `config.js` from the Firefox install folder at startup. Ours
(`autoconfig/config.js`) only maps `resource://shellman-islands/` to `<profile>\chrome\shellman-scripts\` and imports
the four named modules from there; it does not scan for `*.uc.js` or load anything else. `config-prefs.js` (in
`<Firefox>\defaults\pref\`) turns autoconfig on, and sets `general.config.sandbox_enabled=false`: the default
sandbox only allows `pref()` calls, so without that the loader could not reach the profile or import modules.

```powershell
# Close Firefox first. The Firefox folder is under Program Files: run from an elevated PowerShell.
.\install.ps1 -DefaultProfile -Scripts -FirefoxDir "C:\Program Files\Mozilla Firefox"
.\uninstall.ps1 -DefaultProfile        # also removes the scripts and both autoconfig files
```

`-Scripts` needs `-FirefoxDir` given explicitly. If that folder is not writable the script stops with a message
(it never asks for elevation itself) before changing anything. An existing `config.js` / `config-prefs.js` that is
not shellman's is not replaced unless you pass `-Force`; it is then backed up into the profile's backup folder and
put back on uninstall. Uninstall removes the two files only if they are still exactly the ones installed (SHA-256
in `state.json`). Without `-Scripts` the install is CSS-only exactly as before.

**Security trade-off, in plain words.** With the loader installed, Firefox runs our script files with full browser
privileges (the same as Firefox's own code: files, network, other processes through Windows APIs). Anyone or
anything that can write into `<profile>\chrome\shellman-scripts\` could therefore run code inside your browser. That
folder sits inside your profile, which already holds your cookies, saved passwords and extensions, so it is the same
trust boundary as the profile itself, but it is a real new way in and worth knowing. The loader affects every
profile of that Firefox installation only if the profile has a `chrome\shellman-scripts\` folder; other profiles
are untouched. The two files in the Firefox folder can only be written with administrator rights.

**Firefox updates.** The profile files survive updates. The two autoconfig files in the install folder usually
survive normal updates too, but a reinstall, a repair or an update that rewrites the folder can remove them: then
Firefox simply runs without scripts (CSS theme as usual); run `install.ps1 -Scripts ...` again. Firefox internals
the scripts rely on (`#nav-bar`, `sidebar-main` and its shadow DOM, `browser-delayed-startup-finished`) may change
in later versions; on any error the scripts log `[shellman] ...` to the Browser Console (Ctrl+Shift+J) and step back,
leaving the CSS-only layout.

**Turning it off.** `uninstall.ps1`, or just delete `config.js` from the Firefox folder (or the profile's
`chrome\shellman-scripts\` folder) and restart Firefox.

## Real gaps (optional, a DLL)

With the scripts alone the gaps between the islands show Windows' Acrylic, blurred, because Windows paints its
backdrop over the whole window rectangle. `shellman_glass.dll` gives Firefox shellman's own glass instead: the
gaps are truly empty (whatever is behind the window shows through, sharp, as between shellman's own islands)
and each island blurs only what is behind it, with shellman's tint, rim and shadow and smooth corners at any
radius.

```powershell
cargo build --release -p shellman-firefox-glass          # from the repository root
.\install.ps1 -DefaultProfile -Scripts -FirefoxDir "C:\Program Files\Mozilla Firefox" -GlassDll ..\..\target\release\shellman_glass.dll
```

How it works. `chrome/shellman-scripts/shellman-glass.sys.mjs` loads the DLL (from the profile's
`chrome\shellman-scripts\`) with js-ctypes and, for each browser window, on Firefox's main thread:

- turns the window's system backdrop off (`DWMWA_SYSTEMBACKDROP_TYPE` none) and Windows' frame rendering off
  (`DWMWA_NCRENDERING_POLICY` disabled). Measured on Firefox 160 / Windows 11 26300: only with both does every
  pixel Firefox leaves transparent show what is behind the window. With the backdrop off but frame rendering on,
  the area outside Firefox's frame margins is black, and Firefox recomputes those margins on every layout. The
  thin resize border at the sides and bottom is painted clear and still resizes the window;
- attaches shellman's `GlassPanel` (`crates/ui/src/glass`, the same code Files and Settings use) to the window:
  a companion window of the same thread right under it, with one composition visual per island (blurred
  backdrop, tint, rim, shadow). A window subclass moves, sizes, hides and stacks it in the same window message
  that moved the browser window, so there is no second process and no polling;
- measures the islands (toolbar pills, window buttons, tab strip or rail islands, bookmarks toolbar,
  notification bars, sidebar panel, page) in device pixels and sends them when the layout changes (resize and
  mutation observers, window events), at most once per frame and only when they changed. Look changes
  (`appearance.json`, light/dark) restyle the glass.

`:root[shellman-glass]` is set once the DLL is attached; the CSS then hands the islands' fill, hairline and
shadow to the DLL. **Fallback:** no DLL, a DLL of another ABI version, Windows unable to compose (or
`shellman.glass.enabled` = `false`, read when a window opens): nothing is changed and the Acrylic look stays.
Every DLL entry point catches panics and reports failure instead of unwinding into Firefox.

What was measured (300 % monitor, a busy test background):
- Moving the window (moved from another process every 8 ms, 4 px a step, the row through a pill sampled ~240
  times a second): glass and window were in step in 95 % of samples, one step (one frame) apart in 5 %.
  Resizing from the left edge: mostly in step; the page lags for a frame or two the way it does anyway.
- CPU: no polling and no timers while nothing changes (idle: 62 ms of CPU in 10 s for the whole of Firefox);
  moving the window cost no measurable extra CPU (641 ms with, 734 ms without, 4 s of
  moving, Firefox's main process).

Limits:
- A native DLL runs inside Firefox (the same trust boundary as the scripts, see above, but compiled code). It is
  loaded once and never unloaded while Firefox runs; uninstall with Firefox closed.
- The window has no Windows border or rectangular shadow (the islands have their own shadows), and its corners
  are the islands'. Clicks in the gaps go to Firefox (they drag the window, as in shellman's own apps).
- Maximised windows keep the gaps (sharp desktop between the islands); in F11 the visible toolbars and rail stay
  glass; page/video fullscreen covers everything with the page.
- Popups (menus, panels, the URL bar dropdown) keep Windows' Acrylic; they are separate windows.
- Customize mode was not checked with the DLL (the islands follow whatever toolbar items are visible).

## Options

- **Accent / glass:** with the scripts, follow shellman's own setting automatically. Without them, edit
  `chrome/shellman-look.css` only.
  `--shellman-accent`, `--shellman-on-accent`, `--shellman-island-fill`, `--shellman-island-opacity`.
  Defaults: accent preset 5 (`#0067c0`), glass 1.0 (opacity 0.43, shellman's own clearest tint).
  Everything else derives from those four.
- **Tab layout:** `sidebar.verticalTabs` = `true` (vertical, default in `user.js`) or `false` (horizontal).
  `user.js` forces it at each start; delete that line to manage it yourself.
- **Sidebar side:** Firefox Settings > Sidebar, or `sidebar.position_start` (`true` = left). Not forced.
  Nothing is hardcoded to one side: Firefox orders the panes, the CSS uses symmetric gaps and radii.
- **Backdrop:** `widget.windows.mica.toplevel-backdrop` = `2` Acrylic (default, blurred live wallpaper),
  `1` Mica, `3` Mica Alt, `0` automatic.

## Prefs that matter on 160 (verified)

`widget.windows.mica` turns on the media query `(-moz-windows-mica)` and the system backdrop; with it off the
window is opaque. `widget.windows.mica.popups` does the same for popups. `.toplevel-backdrop` selects the
material. `toolkit.legacyUserProfileCustomizations.stylesheets` is required for userChrome/userContent.
`browser.tabs.allow_transparent_browser` lets about:home show the backdrop.

## Known limits

- The gaps between islands show the Windows **acrylic backdrop of the whole window** (blurred and tinted by
  Windows), not the raw sharp wallpaper the shellman islands show. Tried with scripts (ctypes on the window):
  cutting the gaps out with a window region (`SetWindowRgn`, rounded rectangles from the islands' positions)
  does cut them, but DWM still paints the system backdrop over the whole window rectangle, so the gaps stay
  blurred. Turning the backdrop off (`DWMWA_SYSTEMBACKDROP_TYPE` = none) plus the region gives truly sharp gaps,
  but then the islands have no blur at all (the wallpaper shows sharp through the tint), the window shadow is
  gone and the region corners are not anti-aliased (visible stair steps at 150 %). The undocumented
  `SetWindowCompositionAttribute` blur ignores the region and paints the whole rectangle (black on this build).
  shellman itself gets per-island blur from Windows.UI.Composition visuals in a second window; the optional
  `shellman_glass.dll` does exactly that for Firefox (see [Real gaps](#real-gaps-optional-a-dll)).
- `backdrop-filter` on the pills only blurs Firefox's own content; the real blur is Windows'.
- Pages are opaque; the island fill only shows behind transparent pages and Firefox's own pages.
- The focus ring inside the bookmarks/history panel still uses Firefox's lilac in places (the panel is a
  separate chrome document); everywhere else focus is the accent.
- The maximised window keeps the 12 px margins and the gap; it is not flush to the screen edge on purpose.
- Firefox updates rename internals often (`#sidebar-main`, `#urlbar-background`, `--tab-*` variables, nova
  attributes). Expect to re-check after major releases. The selectors most likely to break first: the
  vertical-tabs rail (`#sidebar-container`, `sidebar-main`), the urlbar dropdown, popup variables.
- Things CSS cannot do: change Firefox's tab/rail icon set, round the OS window shadow, or theme third-party
  extension popups and other websites' pages.
- Without the scripts the window turns into a flat opaque slab while it is not focused (Windows' inactive
  backdrop, see Scripts). The CSS keeps Firefox's own inactive colours transparent, but cannot change that.
- CSS only cannot split the toolbar into several pills (there is no element per group), nor style the sidebar's
  tool row (shadow DOM); those need the scripts. The urlbar dropdown, the overflow menu and the panels are not
  split into pieces.
