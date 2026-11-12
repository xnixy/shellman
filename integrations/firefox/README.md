# shellman Islands for Firefox

Turns the Firefox window into the shellman "Islands" look with only `userChrome.css`, `userContent.css`
and a few prefs: floating glass pills and rounded islands, the Windows backdrop visible between them,
no window frame. No extension, nothing in the Firefox install directory.

Checked on **Firefox 160.0** (Windows 11, release build, default theme, "Nova" UI, sidebar revamp on).

## What you get

- Top: a pill with sidebar toggle, back/forward/reload, the URL bar (a soft inner pill, accent ring when
  focused), extension and menu buttons. With vertical tabs the window buttons sit at the right end of that
  pill; with horizontal tabs they get their own small pill beside the tab pill.
- Tabs, both layouts styled (vertical is the default):
  - Vertical (`sidebar.verticalTabs`): the tab rail is an island. Collapsed it is an icon-rail pill like the
    Settings rail, expanded it is a wider island with titles. Selected tab = accent fill. Pinned tabs, the
    new-tab button and the tool buttons at the bottom share the style. Works with the sidebar on the left
    or the right (see below).
  - Horizontal: a pill of tab chips above the toolbar pill, selected chip = accent fill.
- The page is its own island (22 px radius, hairline border, clipped corners), 10-12 px from everything.
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

## Options

- **Accent / glass:** edit `chrome/shellman-look.css` only (shellman regenerates this file).
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
  Windows), not the raw sharp wallpaper the shellman islands show. Firefox cannot make a per-pixel transparent
  top level window through CSS. Mica instead of Acrylic is calmer but not live.
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
