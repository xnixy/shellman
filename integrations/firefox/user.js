// shellman Islands theme: the prefs the userChrome.css theme needs. Nothing else is changed.

// Without this Firefox ignores chrome/userChrome.css and chrome/userContent.css entirely.
user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);

// Ask Windows 11 for a system backdrop behind the browser window (the bit between the islands).
user_pref("widget.windows.mica", true);

// Which backdrop: 0 = automatic, 1 = Mica, 2 = Acrylic (blurred live wallpaper, closest to shellman's glass), 3 = Mica Alt.
user_pref("widget.windows.mica.toplevel-backdrop", 2);

// Same backdrop for menus, panels and the URL bar dropdown, so popups match the islands.
user_pref("widget.windows.mica.popups", true);

// Let the page area show the backdrop where a page paints no background (about:home, about:newtab).
user_pref("browser.tabs.allow_transparent_browser", true);

// Tab layout: true = Firefox's native vertical tabs (sidebar rail), false = horizontal tab chips.
user_pref("sidebar.verticalTabs", true);
// Sidebar side (true = left, the default; false = right) is not forced: change it in Settings. Both are styled.
// user_pref("sidebar.position_start", false);  // uncomment to force the right-hand side
