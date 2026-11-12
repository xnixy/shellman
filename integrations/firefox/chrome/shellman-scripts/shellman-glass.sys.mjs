/* shellman Islands: real gaps. shellman's own glass under each island, nothing between them.
 *
 * Without this module Firefox shows Windows' Acrylic backdrop over the whole window rectangle, so the
 * gaps between the islands are blurred too. With it, shellman_glass.dll (built from
 * integrations/firefox/glass, the same GlassPanel code shellman's apps use) is loaded with js-ctypes
 * and, for each browser window, on Firefox's main thread (the window's own thread):
 *   - turns Windows' backdrop and frame rendering off for the window, so every transparent chrome pixel
 *     shows what is behind the window, sharp;
 *   - puts a companion window right under it with one composition visual per island: blurred backdrop,
 *     shellman's tint, rim and shadow, smooth corners at any radius. It moves, sizes, hides and stacks
 *     with the browser window in the same window message (a subclass, as in shellman's apps).
 * This module measures the islands (pills, rail islands, page) and sends their rectangles whenever the
 * layout changes (resize and position observers, toolbar and sidebar changes, fullscreen), at most once
 * per frame, only when they changed. No polling.
 *
 * :root[shellman-glass] is set only once the DLL has attached to that window; the CSS then hands the
 * island fill, hairline and shadow to the DLL. If the DLL is missing, of another version, or cannot
 * compose (or anything here throws), nothing changes: Windows' Acrylic look stays.
 *
 * Off switch: the pref shellman.glass.enabled = false (read when a window opens).
 */

import { currentLook, log, logError } from "resource://shellman-islands/shellman-live.sys.mjs";

const DLL = "shellman_glass.dll";
const ABI_VERSION = 1;
const PREF_ENABLED = "shellman.glass.enabled";
const PILL = 9999; // radius for "as round as the rectangle allows"

let native = null; // the DLL's functions; never closed (its window procedures stay installed)

function load() {
  if (native !== null) return native || null;
  native = false;
  const file = Services.dirsvc.get("UChrm", Ci.nsIFile);
  file.append("shellman-scripts");
  file.append(DLL);
  if (!file.exists()) return null; // not installed: the Acrylic look
  const { ctypes } = ChromeUtils.importESModule("resource://gre/modules/ctypes.sys.mjs");
  const lib = ctypes.open(file.path);
  const version = lib.declare("shellman_glass_version", ctypes.default_abi, ctypes.uint32_t);
  if (version() !== ABI_VERSION) {
    log("glass: " + DLL + " is version " + version() + ", this script needs " + ABI_VERSION + "; keeping the Acrylic look");
    return null;
  }
  const Island = new ctypes.StructType("IslandRect", [
    { left: ctypes.float }, { top: ctypes.float }, { right: ctypes.float }, { bottom: ctypes.float },
    { radius: ctypes.float }, { opacity: ctypes.float }, { shadow: ctypes.uint32_t },
  ]);
  const handle = ctypes.voidptr_t;
  native = {
    ctypes,
    Island,
    attach: lib.declare("shellman_glass_attach", ctypes.default_abi, handle, ctypes.intptr_t, ctypes.int32_t, ctypes.uint32_t, ctypes.float),
    setStyle: lib.declare("shellman_glass_set_style", ctypes.default_abi, ctypes.int32_t, handle, ctypes.int32_t, ctypes.uint32_t, ctypes.float),
    setIslands: lib.declare("shellman_glass_set_islands", ctypes.default_abi, ctypes.int32_t, handle, Island.ptr, ctypes.uint32_t),
    refresh: lib.declare("shellman_glass_refresh", ctypes.default_abi, ctypes.int32_t, handle),
    detach: lib.declare("shellman_glass_detach", ctypes.default_abi, ctypes.void_t, handle),
  };
  return native;
}

/* ---------- measuring ---------- */

function shown(el) {
  if (!el || !el.isConnected) return null;
  if (el.checkVisibility && !el.checkVisibility({ visibilityProperty: true })) return null;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0 ? r : null;
}

function radiusOf(win, el) {
  return parseFloat(win.getComputedStyle(el).borderTopLeftRadius) || 0;
}

function union(rects) {
  const list = rects.filter(Boolean);
  if (!list.length) return null;
  return {
    left: Math.min(...list.map((r) => r.left)), top: Math.min(...list.map((r) => r.top)),
    right: Math.max(...list.map((r) => r.right)), bottom: Math.max(...list.map((r) => r.bottom)),
  };
}

/** The islands of one window, in CSS pixels of its document: [{ left, top, right, bottom, radius }]. */
export function measure(win) {
  const doc = win.document;
  const root = doc.documentElement;

  const out = [];
  const add = (rect, radius) => { if (rect) out.push({ left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, radius }); };
  const el = (sel) => doc.querySelector(sel);
  const navBar = el("#nav-bar");
  const target = el("#nav-bar-customization-target");

  // Toolbar: with the pills script each labelled run is a pill (the address bar a pill of its own),
  // without it all toolbar items and the menu button are one pill.
  if (root.hasAttribute("shellman-pills")) {
    let run = [];
    for (const item of doc.querySelectorAll("#nav-bar [shellman-pill]")) {
      if (item.hasAttribute("shellman-first")) run = [];
      run.push(shown(item));
      if (item.hasAttribute("shellman-last")) add(union(run), PILL);
    }
  } else if (navBar && target) {
    const parts = [shown(target)];
    let after = false;
    for (const child of navBar.children) {
      if (child === target) { after = true; continue; }
      if (after && !child.classList.contains("titlebar-buttonbox-container") && !child.classList.contains("titlebar-spacer")) parts.push(shown(child));
    }
    add(union(parts), PILL);
  }
  // Window buttons, wherever Firefox put them (nav bar, or the tab strip with horizontal tabs).
  for (const box of doc.querySelectorAll(".titlebar-buttonbox-container")) add(shown(box), PILL);
  // Horizontal tab strip and the bookmarks toolbar.
  const vertical = el("#tabbrowser-tabs")?.getAttribute("orient") === "vertical";
  if (!vertical) add(shown(el("#TabsToolbar > .toolbar-items")), PILL);
  add(shown(el("#PersonalToolbar")), PILL);

  // The rail: split into islands by the pills script, else one island.
  const side = el("#sidebar-container");
  if (shown(side)) {
    if (root.hasAttribute("shellman-pills")) {
      if (vertical) {
        const pinned = el("#pinned-tabs-container");
        if (pinned && pinned.querySelector(".tabbrowser-tab")) add(shown(pinned), radiusOf(win, pinned));
        const list = el("#tabbrowser-arrowscrollbox");
        if (list) add(shown(list), radiusOf(win, list));
        const newTab = el("#tabbrowser-tabs > #vertical-tabs-newtab-button");
        if (newTab) add(shown(newTab), PILL);
      }
      const tools = el("sidebar-main")?.shadowRoot?.querySelector(".buttons-wrapper");
      if (tools) add(shown(tools), radiusOf(win, tools));
    } else {
      add(shown(side), radiusOf(win, side));
    }
  }
  // Notification bars above the page (session restore, "Firefox is not your default browser", ...).
  for (const bar of doc.querySelectorAll("notification-message")) add(shown(bar), Math.max(radiusOf(win, bar), 22));
  const panel = el("#sidebar-box");
  if (panel) add(shown(panel), radiusOf(win, panel));
  const page = el("#tabbrowser-tabbox");
  if (page) add(shown(page), radiusOf(win, page));
  return out;
}

/** CSS-pixel islands to the DLL's device-pixel rectangles (rounding is the DLL's). */
export function toDevice(islands, scale) {
  return islands.map((i) => ({
    left: i.left * scale, top: i.top * scale, right: i.right * scale, bottom: i.bottom * scale,
    radius: i.radius * scale, opacity: 1, shadow: 1,
  }));
}

/* ---------- one window ---------- */

function hwndOf(win) {
  return win.docShell.treeOwner.QueryInterface(Ci.nsIBaseWindow).nativeHandle;
}

function setUp(win) {
  if (win.__shellmanGlass !== undefined) return;
  win.__shellmanGlass = null;
  const doc = win.document;
  const root = doc.documentElement;
  if (root.getAttribute("windowtype") !== "navigator:browser") return;
  if (!Services.prefs.getBoolPref(PREF_ENABLED, true)) return;
  const n = load();
  if (!n) return;

  const lightQuery = win.matchMedia("(prefers-color-scheme: light)");
  const look = () => { const l = currentLook(); return [lightQuery.matches ? 1 : 0, l.rgb >>> 0, l.glass]; };
  const handle = n.attach(n.ctypes.intptr_t(n.ctypes.Int64(hwndOf(win))), ...look());
  if (handle.isNull()) {
    log("glass: Windows cannot compose glass here; keeping the Acrylic look");
    return;
  }
  win.__shellmanGlass = handle;
  root.setAttribute("shellman-glass", "true");

  let last = "";
  let queued = false;
  const send = () => {
    queued = false;
    if (win.closed || !win.__shellmanGlass) return;
    try {
      const list = toDevice(measure(win), win.devicePixelRatio);
      const key = JSON.stringify(list);
      if (key === last) return;
      last = key;
      const arr = n.Island.array(list.length)(list.map((i) => new n.Island(i.left, i.top, i.right, i.bottom, i.radius, i.opacity, i.shadow)));
      n.setIslands(handle, list.length ? arr[0].address() : null, list.length);
    } catch (e) {
      logError("glass: measuring", e);
    }
  };
  const queue = () => {
    if (queued) return;
    queued = true;
    win.requestAnimationFrame(send);
  };
  // Once now (next frame), and once more when a burst of changes has settled, for moves that end a
  // transition without changing any observed size.
  let settle = null;
  const schedule = () => {
    if (settle) win.clearTimeout(settle);
    settle = win.setTimeout(() => { settle = null; queue(); }, 300);
    queue();
  };
  const restyle = () => {
    try { if (win.__shellmanGlass) n.setStyle(handle, ...look()); } catch (e) { logError("glass: style", e); }
  };
  const refresh = () => {
    try { if (win.__shellmanGlass) n.refresh(handle); } catch (e) { logError("glass: refresh", e); }
    schedule();
  };

  // What moves islands: sizes (resize observer), toolbar/sidebar contents and labels (mutations),
  // and window states.
  const ro = new win.ResizeObserver(schedule);
  const watchSizes = () => {
    for (const sel of ["#navigator-toolbox", "#nav-bar", "#nav-bar-customization-target", "#urlbar-container", "#TabsToolbar",
      "#PersonalToolbar", "#browser", "#sidebar-container", "#sidebar-box", "#tabbrowser-tabbox", "#tabbrowser-tabs",
      "#pinned-tabs-container", "#tabbrowser-arrowscrollbox", "#vertical-tabs-newtab-button"]) {
      const e = doc.querySelector(sel);
      if (e) ro.observe(e);
    }
    for (const e of doc.querySelectorAll("#nav-bar [shellman-pill], .titlebar-buttonbox-container")) ro.observe(e);
    const tools = doc.querySelector("sidebar-main")?.shadowRoot?.querySelector(".buttons-wrapper");
    if (tools) ro.observe(tools);
  };
  watchSizes();
  const mo = new win.MutationObserver(() => { watchSizes(); schedule(); });
  mo.observe(root, { attributes: true, attributeFilter: ["sizemode", "inFullscreen", "inDOMFullscreen", "customizing", "shellman-pills"] });
  for (const sel of ["#navigator-toolbox", "#browser"]) {
    const e = doc.querySelector(sel);
    if (e) mo.observe(e, { childList: true, subtree: true, attributes: true, attributeFilter: ["hidden", "collapsed", "shellman-pill", "shellman-first", "shellman-last", "expanded", "orient", "sidebar-positionend"] });
  }
  // The panes of #browser are reordered (sidebar left or right) by their inline style alone, and in
  // fullscreen the toolbox slides in and out the same way.
  const browser = doc.getElementById("browser");
  for (const pane of [...(browser ? browser.children : []), doc.getElementById("navigator-toolbox")]) {
    if (pane) mo.observe(pane, { attributes: true, attributeFilter: ["style"] });
  }
  const events = ["resize", "fullscreen", "aftercustomization", "toolbarvisibilitychange", "TabPinned", "TabUnpinned", "transitionend"];
  for (const type of events) win.addEventListener(type, schedule);
  win.addEventListener("sizemodechange", refresh);
  win.addEventListener("activate", refresh);
  lightQuery.addEventListener("change", restyle);
  const observer = {
    observe(_s, topic) {
      if (topic === "shellman-look-changed") restyle();
      else if (topic === "look-and-feel-changed") { restyle(); win.setTimeout(refresh, 0); }
    },
  };
  Services.obs.addObserver(observer, "shellman-look-changed");
  Services.obs.addObserver(observer, "look-and-feel-changed");
  // sidebar-main renders its shadow root after the window is up.
  win.setTimeout(() => { watchSizes(); schedule(); }, 500);
  win.setTimeout(() => { watchSizes(); schedule(); }, 2000);

  win.addEventListener("unload", () => {
    ro.disconnect();
    mo.disconnect();
    Services.obs.removeObserver(observer, "shellman-look-changed");
    Services.obs.removeObserver(observer, "look-and-feel-changed");
    const h = win.__shellmanGlass;
    win.__shellmanGlass = null;
    try { if (h) n.detach(h); } catch (e) { logError("glass: detach", e); }
  }, { once: true });
  schedule();
}

const startup = {
  observe(subject, topic) {
    if (topic === "browser-delayed-startup-finished") {
      try { setUp(subject); } catch (e) { logError("glass", e); }
    }
  },
};

let started = false;
export function start() {
  if (started) return;
  started = true;
  Services.obs.addObserver(startup, "browser-delayed-startup-finished");
  for (const win of Services.wm.getEnumerator("navigator:browser")) {
    if (win.gBrowserInit && win.gBrowserInit.delayedStartupFinished) {
      try { setUp(win); } catch (e) { logError("glass", e); }
    }
  }
}
