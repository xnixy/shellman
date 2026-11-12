/* shellman Islands: more, smaller islands than CSS alone can make.
 *
 * The toolbar keeps every widget where Firefox (and the user, via Customize) put it. This module only
 * labels them: each visible item in the navigation toolbar gets shellman-pill="nav" (back, forward,
 * reload, stop, home), "address" (URL bar, search bar) or "actions" (everything else, the menu
 * included), plus shellman-first / shellman-last at the ends of each run of the same kind. The CSS
 * (shellman-islands.css, under :root[shellman-pills]) paints each run as its own pill. Nothing is moved,
 * so customisation, overflow and the menus keep working; the labels are recomputed when the toolbar
 * changes.
 *
 * The vertical-tabs tool row (bookmarks, history, ...) lives in sidebar-main's shadow DOM, which
 * userChrome.css cannot reach; a small style sheet is adopted into that shadow root to make it a pill.
 *
 * :root[shellman-pills] is set only when this has run without error in that window. If anything is
 * missing or throws, the attribute is removed and the plain CSS layout (one toolbar pill, one sidebar
 * island) applies.
 */

import { log, logError } from "resource://shellman-islands/shellman-live.sys.mjs";

const NAV = new Set(["back-button", "forward-button", "stop-reload-button", "reload-button", "stop-button", "home-button"]);
const ADDRESS = new Set(["urlbar-container", "search-container"]);
const NOT_A_PILL = new Set(["toolbarspring", "toolbarspacer", "toolbarseparator"]);
const ATTRS = ["shellman-pill", "shellman-first", "shellman-last"];

const SHADOW_CSS = `
  .buttons-wrapper {
    background: var(--sm-fill);
    border: 1px solid var(--sm-hair);
    border-radius: var(--sm-radius, 22px);
    box-shadow: var(--sm-shadow);
    margin-top: 0;
    padding-block: 6px;
    box-sizing: border-box;
  }
  :host(:not([expanded])) .buttons-wrapper { border-radius: 28px; }
  #sidebar-tools-and-extensions-splitter { opacity: 0; }
`;

function kindOf(item) {
  if (ADDRESS.has(item.id)) return "address";
  if (NAV.has(item.id)) return "nav";
  return "actions";
}

function visible(win, el) {
  if (el.hidden || el.collapsed) return false;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0;
}

/** Label the navigation toolbar's items by run. */
function group(win) {
  const doc = win.document;
  const navBar = doc.getElementById("nav-bar");
  const target = doc.getElementById("nav-bar-customization-target");
  if (!navBar || !target) throw new Error("nav-bar not found");
  // whole document: widgets that moved to another toolbar (e.g. switching tab layout) must lose their labels too
  for (const el of doc.querySelectorAll("[shellman-pill]")) for (const a of ATTRS) el.removeAttribute(a);
  if (doc.documentElement.hasAttribute("customizing")) return; // Customize mode shows the plain layout

  // Visual order: the target's items, then nav-bar's own items after it (the menu button), but not
  // the window buttons, which are a pill of their own in CSS.
  const seq = [];
  for (const el of target.children) seq.push(el);
  let after = false;
  for (const el of navBar.children) {
    if (el === target) { after = true; continue; }
    if (after && !el.classList.contains("titlebar-buttonbox-container") && !el.classList.contains("titlebar-spacer")) seq.push(el);
  }

  let run = null; // { kind, items }
  const runs = [];
  for (const el of seq) {
    if (!visible(win, el)) continue;
    if (NOT_A_PILL.has(el.localName)) { run = null; continue; } // a spring or space splits pills
    const kind = kindOf(el);
    if (!run || run.kind !== kind || kind === "address") { run = { kind, items: [] }; runs.push(run); }
    run.items.push(el);
  }
  for (const r of runs) {
    for (const el of r.items) el.setAttribute("shellman-pill", r.kind);
    r.items[0].setAttribute("shellman-first", "true");
    r.items[r.items.length - 1].setAttribute("shellman-last", "true");
  }
}

function adoptShadowSheet(win) {
  const host = win.document.querySelector("sidebar-main");
  const root = host && host.shadowRoot;
  if (!root) return false;
  if (root.adoptedStyleSheets.some((s) => s.__shellman)) return true;
  const sheet = new win.CSSStyleSheet();
  sheet.replaceSync(SHADOW_CSS);
  sheet.__shellman = true;
  root.adoptedStyleSheets = [...root.adoptedStyleSheets, sheet];
  return true;
}

function setUp(win) {
  if (win.__shellmanPills) return;
  const doc = win.document;
  if (doc.documentElement.getAttribute("windowtype") !== "navigator:browser") return;
  win.__shellmanPills = true;
  const root = doc.documentElement;
  const fail = (where, e) => {
    root.removeAttribute("shellman-pills");
    logError("pills: " + where, e);
  };

  let queued = false;
  const regroup = () => {
    if (queued) return;
    queued = true;
    win.requestAnimationFrame(() => {
      queued = false;
      try {
        group(win);
        if (!adoptShadowSheet(win) && doc.querySelector("sidebar-main")) return; // shadow root not ready yet
      } catch (e) {
        fail("grouping", e);
      }
    });
  };

  try {
    group(win);
    adoptShadowSheet(win);
    root.setAttribute("shellman-pills", "true");
  } catch (e) {
    fail("setup", e);
    return;
  }

  // Re-label when items are added, removed, hidden or overflowed, and after Customize.
  const mo = new win.MutationObserver(regroup);
  const navBar = doc.getElementById("nav-bar");
  mo.observe(navBar, { childList: true, subtree: true, attributes: true, attributeFilter: ["hidden", "collapsed", "overflowedItem"] });
  const side = doc.getElementById("sidebar-container");
  if (side) mo.observe(side, { childList: true });
  for (const type of ["aftercustomization", "customizationending", "resize", "toolbarvisibilitychange"]) win.addEventListener(type, regroup);
  win.addEventListener("unload", () => mo.disconnect(), { once: true });
  // sidebar-main may render its shadow root after the window is up.
  win.setTimeout(regroup, 500);
  win.setTimeout(regroup, 2000);
}

const observer = {
  observe(subject, topic) {
    if (topic === "browser-delayed-startup-finished") {
      try { setUp(subject); } catch (e) { logError("pills", e); }
    }
  },
};

let started = false;
export function start() {
  if (started) return;
  started = true;
  Services.obs.addObserver(observer, "browser-delayed-startup-finished");
  for (const win of Services.wm.getEnumerator("navigator:browser")) {
    if (win.gBrowserInit && win.gBrowserInit.delayedStartupFinished) {
      try { setUp(win); } catch (e) { logError("pills", e); }
    }
  }
  log("pills ready");
}
