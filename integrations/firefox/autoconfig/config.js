// shellman Islands loader (Firefox skips this first line). Installed next to firefox.exe by install.ps1 -Scripts.
//
// Runs once at startup with full browser privileges. It does exactly this, nothing more:
//   1. maps resource://shellman-islands/ to <profile>\chrome\shellman-scripts\ (only that folder),
//   2. imports the named modules below from it and calls their start().
// No *.uc.js scanning, no other folder. If the folder or a file is missing, or anything throws, Firefox
// carries on normally and the CSS theme still applies. Messages go to the Browser Console as "[shellman] ...".
try {
  (function () {
    const MODULES = ["shellman-live.sys.mjs", "shellman-pills.sys.mjs", "shellman-window.sys.mjs", "shellman-glass.sys.mjs"];
    const log = (msg) => {
      try { Services.console.logStringMessage("[shellman] " + msg); } catch (_) {}
    };
    let dir;
    try {
      dir = Services.dirsvc.get("UChrm", Ci.nsIFile);
      dir.append("shellman-scripts");
    } catch (e) {
      log("loader: no profile chrome folder (" + e + ")");
      return;
    }
    if (!dir.exists() || !dir.isDirectory()) return; // scripts not installed in this profile: CSS only
    try {
      Services.io.getProtocolHandler("resource").QueryInterface(Ci.nsIResProtocolHandler)
        .setSubstitution("shellman-islands", Services.io.newFileURI(dir));
    } catch (e) {
      log("loader: " + e);
      return;
    }
    for (const name of MODULES) {
      try {
        const file = dir.clone();
        file.append(name);
        if (!file.exists() || !file.isFile()) continue;
        const mod = ChromeUtils.importESModule("resource://shellman-islands/" + name);
        if (typeof mod.start === "function") mod.start();
      } catch (e) {
        log("loader: " + name + " failed: " + e + (e && e.lineNumber ? " (line " + e.lineNumber + ")" : ""));
      }
    }
  })();
} catch (e) {
  try { Services.console.logStringMessage("[shellman] loader: " + e); } catch (_) {}
}
