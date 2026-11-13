# Islands

shellman's own icon pack and its default: the app tiles of the design mockups
(`docs/design/desktop-concepts.html`, the dock). Every icon is a rounded tile with a soft
diagonal gradient, a faint top highlight and a white line glyph.

It is compiled into every shellman app (`crates/core/build.rs` reads this folder), so it needs no
installing. The folder is also an ordinary freedesktop icon theme: Settings › Appearance ›
Icon pack › *Add pack from folder…* takes it as it is, and other launchers and docks that read
icon themes can use it too.

## Layout

    index.theme              the theme's name and its two directories
    scalable/apps/<name>.svg the tile, for 32 px and up
    24x24/apps/<name>.svg    the same tile for 16-24 px: glyph 1.2x larger, line 2.6 instead of 1.8

Both are written by `python tools/islands/make.py`, which holds every glyph and colour. Edit the
script, not the SVGs, and run it again.

## Grid and style, for adding an icon that matches

- Tile: 46 x 46, corner radius 13, filling the whole icon (no margin: the dock and launcher show a
  pack icon edge to edge).
- Fill: a linear gradient as CSS draws `linear-gradient(<angle>, a, b)` on the tile: 160 degrees
  (top a little left to bottom a little right) for most, 135 degrees (corner to corner) for the
  warmer ones. Two stops; three only where an app's own colours need them.
- Highlight: the CSS `inset 0 1px 0 #ffffff33`, a 1-unit white line at 20% along the inside of the
  top edge, following the corners.
- Glyph: drawn on a 24 x 24 grid, placed at (11, 11); stroke 1.8, round caps and joins, no fill,
  white. Solid parts are small dots only (r 0.9 to 2). Keep the glyph inside 3..21 on both axes
  and centred by eye: its visual weight, not its box, sits in the middle.
- Colour: pick from the palette in `make.py` (the mockup's own twelve tiles, plus a few in the
  same key). System tools are neutral (steel, graphite, night); nothing is neon. Every tile must
  hold a white glyph and read on both dark and light glass.
- Small variant: the same drawing; the script scales it. If a glyph turns to mush at 16 px,
  simplify it rather than thinning it.
- Look at it whole: `cargo run -p shellman-core --example islands_sheet -- <dir>` draws a contact
  sheet at 96, 48, 24 and 16 px on dark and light, the small sizes enlarged, and each app tile next
  to that app's real icon from the machine.

## How names map to apps

A file's name is the name an app is looked up by (`shellman-core`, `icon_pack/names.rs`):

- shellman's own apps: `shellman-<app>` for `shellman-<app>.exe` (`shellman-files`,
  `shellman-settings`, `shellman-tasks`, `shellman-terminal`, `shellman-launcher`,
  `shellman-control`). Only exact names count, so packs not made for shellman leave them alone.
- The Recycle Bin: `user-trash` and `user-trash-full`.
- Kinds of app, by their freedesktop names: `web-browser`, `utilities-terminal`,
  `accessories-text-editor`, `system-file-manager`, `internet-mail`, `office-calendar`,
  `internet-chat`, `multimedia-audio-player`, `multimedia-video-player`,
  `multimedia-photo-manager`, `applications-games`, `accessories-calculator`, `camera`, `clock`,
  `accessories-notes`, `maps`, `software-store`, `preferences-system`, `utilities-system-monitor`,
  `archive-manager`, `document-viewer`, `code-editor`, `preferences-desktop-remote-desktop`,
  `virtual-machine`, `accessories-screenshot-tool`, `accessories-drawing`,
  `audio-input-microphone`, `download-manager`, `password-manager`, `x-office-document`,
  `x-office-spreadsheet`, `x-office-presentation`, `preferences-desktop-accessibility`,
  `administration`, `preferences-system-privacy`. Windows' own apps and many others reach these
  through the alias table (`notepad` is `accessories-text-editor`, `taskmgr` is
  `utilities-system-monitor`, Discord falls back to `internet-chat` in a pack without `discord`).
- Well-known apps, by the names packs usually give them: `firefox`, `google-chrome`,
  `microsoft-edge`, `brave-browser`, `discord`, `steam`, `spotify`, `visual-studio-code`, `slack`,
  `telegram`, `whatsapp`, `zoom`, `com.obsproject.studio`, `vlc`, `notion`, `obsidian`,
  `github-desktop`, `qbittorrent`, `1password`, `tailscale`, `zed`, `git`, `xbox`.

Apps that match none of these keep their own icon.

## The apps' marks

The tiles for well-known apps are independent, stylised icons drawn for this pack, in its style,
only so that the apps can be told apart at a glance. They are not the apps' logos, are not
official, and are not endorsed by or affiliated with the apps' makers. App names and marks belong
to their owners.

## Licence

MIT, as the rest of shellman (the `license` in the workspace's `Cargo.toml`).
