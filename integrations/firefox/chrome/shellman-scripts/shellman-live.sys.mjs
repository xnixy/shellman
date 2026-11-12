/* shellman Islands: the live look.
 *
 * Reads shellman's appearance.json (accent + glass amount) and lays the matching values of
 * --shellman-accent, --shellman-on-accent, --shellman-island-fill and --shellman-island-opacity over
 * shellman-look.css, in every browser window and popup at once, through one user style sheet
 * (nsIStyleSheetService). Re-applies when the file changes (a folder change notification waited on in
 * a worker thread) and, for the "System" accent, when Windows' colours change. Light/dark is a media
 * query inside the sheet, so it follows Windows the same way the CSS theme does.
 *
 * Loaded once per browser process by config.js. Every step is guarded: if anything here fails, the
 * plain CSS theme (shellman-look.css defaults) stays as it is.
 */

import { setTimeout, clearTimeout, setInterval, clearInterval } from "resource://gre/modules/Timer.sys.mjs";

const TAG = "[shellman]";
const PREF_PATH = "shellman.appearance.path";
const WORKER_URL = "resource://shellman-islands/shellman-watch.worker.js";
const FALLBACK_MS = 3000;
const DEBOUNCE_MS = 150;

// crates/ui/src/theme/mod.rs PRESETS, same order.
const PRESETS = [
  [196, 54, 78], [214, 82, 52], [204, 132, 0], [28, 138, 76], [0, 128, 128],
  [0, 103, 192], [84, 82, 214], [136, 64, 200], [96, 108, 128],
];
const FILL_CONTRAST = 1.6;

export function log(msg) {
  try { Services.console.logStringMessage(TAG + " " + msg); } catch (_) {}
}

export function logError(where, e) {
  try {
    const err = Cc["@mozilla.org/scripterror;1"].createInstance(Ci.nsIScriptError);
    err.init(TAG + " " + where + ": " + e, (e && e.fileName) || "shellman-live.sys.mjs", null,
      (e && e.lineNumber) || 0, 0, Ci.nsIScriptError.errorFlag, "chrome javascript");
    Services.console.logMessage(err);
  } catch (_) {
    log(where + ": " + e);
  }
}

/* ---------- colours: a port of the parts of crates/ui/src/theme that the CSS needs ---------- */

const gray = (v) => [v, v, v];
const mix = (a, b, t) => a.map((x, i) => Math.round(x + (b[i] - x) * t));
function luminance(c) {
  const ch = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
  return 0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2]);
}
function contrast(a, b) {
  const la = luminance(a), lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}
const hex = (c) => "#" + c.map((v) => v.toString(16).padStart(2, "0")).join("");

/** The Windows accent, from the registry value the taskbar uses (0xAABBGGRR), like system_accent(). */
function systemAccent() {
  try {
    const key = Cc["@mozilla.org/windows-registry-key;1"].createInstance(Ci.nsIWindowsRegKey);
    key.open(Ci.nsIWindowsRegKey.ROOT_KEY_CURRENT_USER, "Software\\Microsoft\\Windows\\DWM", Ci.nsIWindowsRegKey.ACCESS_READ);
    try {
      const v = key.readIntValue("AccentColor") >>> 0;
      return [v & 0xff, (v >>> 8) & 0xff, (v >>> 16) & 0xff];
    } finally {
      key.close();
    }
  } catch (_) {
    return null;
  }
}

function resolveAccent(choice) {
  if (choice && typeof choice === "object") {
    if (Number.isInteger(choice.Preset)) return PRESETS[choice.Preset] || PRESETS[0];
    if (Array.isArray(choice.Custom) && choice.Custom.length === 3 && choice.Custom.every((v) => Number.isInteger(v) && v >= 0 && v <= 255)) {
      return choice.Custom;
    }
  }
  return systemAccent() || PRESETS[0]; // "System", or anything unknown
}

/** CSS variable values for one mode, as theme::Colors::new and theme::shell compute them. */
function modeVars(light, accent, glass) {
  const tint = (g, t) => mix(gray(g), accent, t);
  const bg = light ? tint(242, 0.05) : tint(16, 0.06);
  const text = light ? tint(28, 0.06) : tint(240, 0.03);
  const fill = contrast(accent, bg) >= FILL_CONTRAST ? accent : text;
  const onAccent = luminance(fill) > 0.4 ? [20, 16, 20] : [255, 255, 255];
  const island = mix(gray(light ? 238 : 10), accent, 0.06);
  const alpha = Math.round(225 + (110 - 225) * glass) / 255;
  return [
    `--shellman-accent: ${hex(fill)} !important;`,
    `--shellman-on-accent: ${hex(onAccent)} !important;`,
    `--shellman-island-fill: ${hex(island)} !important;`,
    `--shellman-island-opacity: ${alpha.toFixed(3)} !important;`,
  ].join(" ");
}

export function lookCss(appearance) {
  const accent = resolveAccent(appearance.accent);
  let glass = Number(appearance.glass);
  if (!Number.isFinite(glass)) glass = 1.0;
  glass = Math.min(1, Math.max(0, glass));
  // Only Firefox's own documents (window, popups, sidebar panels, about: pages); never websites.
  return `@-moz-document url-prefix("chrome:"), url-prefix("about:") {
  :root { ${modeVars(false, accent, glass)} }
  @media (prefers-color-scheme: light) { :root { ${modeVars(true, accent, glass)} } }
}`;
}

/* ---------- applying ---------- */

let sheetUri = null;
let lastCss = null;
let lastAppearance = null;

function applyCss(css) {
  if (css === lastCss) return;
  const sss = Cc["@mozilla.org/content/style-sheet-service;1"].getService(Ci.nsIStyleSheetService);
  const uri = Services.io.newURI("data:text/css;charset=utf-8," + encodeURIComponent("/* shellman live look */\n" + css));
  sss.loadAndRegisterSheet(uri, sss.USER_SHEET);
  if (sheetUri && sss.sheetRegistered(sheetUri, sss.USER_SHEET)) sss.unregisterSheet(sheetUri, sss.USER_SHEET);
  sheetUri = uri;
  lastCss = css;
}

function clearCss() {
  if (!sheetUri) return;
  const sss = Cc["@mozilla.org/content/style-sheet-service;1"].getService(Ci.nsIStyleSheetService);
  if (sss.sheetRegistered(sheetUri, sss.USER_SHEET)) sss.unregisterSheet(sheetUri, sss.USER_SHEET);
  sheetUri = null;
  lastCss = null;
}

export function appearancePath() {
  const custom = Services.prefs.getStringPref(PREF_PATH, "");
  if (custom) return custom;
  return PathUtils.join(Services.dirsvc.get("AppData", Ci.nsIFile).path, "shellman", "appearance.json");
}

let reading = null;
async function reload(retry = true) {
  const path = appearancePath();
  let text;
  try {
    text = await IOUtils.readUTF8(path);
  } catch (e) {
    if (e && e.name === "NotFoundError") {
      // No shellman look saved (yet): the CSS defaults apply.
      if (lastAppearance) log("appearance file gone; back to the CSS defaults");
      lastAppearance = null;
      clearCss();
      return;
    }
    throw e;
  }
  let appearance;
  try {
    appearance = JSON.parse(text);
    if (!appearance || typeof appearance !== "object") throw new Error("not an object");
  } catch (e) {
    // Most likely caught mid-write: try once more shortly, else keep the last good look.
    if (retry) {
      await new Promise((r) => setTimeout(r, 300));
      return reload(false);
    }
    log("appearance.json unreadable, keeping the current look: " + e);
    return;
  }
  lastAppearance = appearance;
  applyCss(lookCss(appearance));
}

let pending = null;
function scheduleReload(delay = DEBOUNCE_MS) {
  if (pending) clearTimeout(pending);
  pending = setTimeout(() => {
    pending = null;
    if (reading) { scheduleReload(delay); return; }
    reading = reload().catch((e) => logError("reading appearance", e)).finally(() => { reading = null; });
  }, delay);
}

/* ---------- watching ---------- */

let worker = null;
let stopEvent = null;
let kernel32 = null;
let setEvent = null;
let fallbackTimer = null;
let fallbackStamp = null;
let watchMode = "none";

function startFallback(reason) {
  if (fallbackTimer) return;
  watchMode = "timer";
  log("watching appearance.json with a " + FALLBACK_MS / 1000 + " s timer (" + reason + ")");
  fallbackTimer = setInterval(async () => {
    try {
      const path = appearancePath();
      let stamp = "missing";
      try { stamp = String((await IOUtils.stat(path)).lastModified); } catch (_) {}
      if (fallbackStamp !== null && stamp !== fallbackStamp) scheduleReload(0);
      fallbackStamp = stamp;
      // The folder appeared later (shellman installed after Firefox started): switch to the notification.
      if (!worker && (await IOUtils.exists(PathUtils.parent(path)))) startWatcher();
    } catch (e) {
      logError("timer", e);
    }
  }, FALLBACK_MS);
}

function stopFallback() {
  if (fallbackTimer) clearInterval(fallbackTimer);
  fallbackTimer = null;
}

async function startWatcher() {
  const dir = PathUtils.parent(appearancePath());
  if (!(await IOUtils.exists(dir))) {
    startFallback("folder " + dir + " does not exist yet");
    return;
  }
  try {
    const { ctypes } = ChromeUtils.importESModule("resource://gre/modules/ctypes.sys.mjs");
    if (!kernel32) {
      kernel32 = ctypes.open("kernel32.dll");
      const CreateEventW = kernel32.declare("CreateEventW", ctypes.winapi_abi, ctypes.voidptr_t,
        ctypes.voidptr_t, ctypes.int32_t, ctypes.int32_t, ctypes.char16_t.ptr);
      stopEvent = CreateEventW(null, 1, 0, null); // manual reset, not signalled
      if (stopEvent.isNull()) throw new Error("CreateEventW failed");
      setEvent = kernel32.declare("SetEvent", ctypes.winapi_abi, ctypes.int32_t, ctypes.voidptr_t);
    }
    const w = new ChromeWorker(WORKER_URL);
    worker = w;
    w.onmessage = (m) => {
      const d = m.data || {};
      if (d.type === "changed") scheduleReload();
      else if (d.type === "ready") { watchMode = "notification"; stopFallback(); }
      else if (d.type === "error") { logError("watcher", d.message); dropWorker(w); startFallback("change notification failed"); }
      else if (d.type === "stopped") dropWorker(w);
    };
    w.onerror = (e) => {
      logError("watcher", e.message);
      dropWorker(w);
      startFallback("worker error");
    };
    w.postMessage({ dir, stop: ctypes.cast(stopEvent, ctypes.uintptr_t).value.toString() });
  } catch (e) {
    logError("starting the watcher", e);
    startFallback("no change notification");
  }
}

function dropWorker(w) {
  if (worker === w) worker = null;
  try { w.terminate(); } catch (_) {}
}

function shutdown() {
  try { if (setEvent && stopEvent) setEvent(stopEvent); } catch (_) {}
  if (worker) dropWorker(worker);
  stopFallback();
}

/** For tests and the README: how the file is being watched. */
export function status() {
  return { watchMode, path: appearancePath(), applied: lastCss !== null, appearance: lastAppearance };
}

const observer = {
  observe(subject, topic) {
    try {
      if (topic === "look-and-feel-changed") {
        // Windows accent/colours changed: only matters for the "System" accent.
        const a = lastAppearance && lastAppearance.accent;
        if (lastAppearance && !(a && typeof a === "object")) applyCss(lookCss(lastAppearance));
      } else if (topic === "quit-application" || topic === "profile-before-change") {
        Services.obs.removeObserver(observer, "look-and-feel-changed");
        Services.obs.removeObserver(observer, "quit-application");
        Services.obs.removeObserver(observer, "profile-before-change");
        shutdown();
      }
    } catch (e) {
      logError(topic, e);
    }
  },
};

let started = false;
export function start() {
  if (started) return;
  started = true;
  try {
    Services.obs.addObserver(observer, "look-and-feel-changed");
    Services.obs.addObserver(observer, "quit-application");
    Services.obs.addObserver(observer, "profile-before-change");
  } catch (e) {
    logError("observers", e);
  }
  scheduleReload(0);
  startWatcher().catch((e) => { logError("watcher", e); startFallback("watcher failed"); });
}
