/* shellman Islands: keep the glass when the browser window is not focused.
 *
 * Windows draws the system backdrop (Acrylic, Mica, Mica Alt alike) as a flat solid colour while a
 * window is inactive. That slab is DWM's, not Firefox's: every chrome background is already
 * transparent, so no CSS can change it (measured on Firefox 160, Windows 11 26300). DWM follows the
 * window's non-client activation state, so when the window loses focus this sends it
 * WM_NCACTIVATE(TRUE): DWM keeps the live backdrop, Firefox's own focus handling (WM_ACTIVATE) is
 * untouched, and the next activation proceeds as usual. One message per focus loss, nothing else.
 */

import { logError } from "resource://shellman-islands/shellman-live.sys.mjs";

const WM_NCACTIVATE = 0x86;
let native = null;

function api() {
  if (native) return native;
  const { ctypes } = ChromeUtils.importESModule("resource://gre/modules/ctypes.sys.mjs");
  const user32 = ctypes.open("user32.dll");
  const send = user32.declare("SendMessageW", ctypes.winapi_abi, ctypes.intptr_t,
    ctypes.voidptr_t, ctypes.uint32_t, ctypes.uintptr_t, ctypes.intptr_t);
  const hwnd = (win) => ctypes.voidptr_t(ctypes.UInt64(win.docShell.treeOwner.QueryInterface(Ci.nsIBaseWindow).nativeHandle));
  native = { send, hwnd };
  return native;
}

function keepActiveLook(win) {
  try {
    if (win.closed || win.windowState === win.STATE_MINIMIZED) return;
    const { send, hwnd } = api();
    send(hwnd(win), WM_NCACTIVATE, 1, 0);
  } catch (e) {
    logError("window glass", e);
  }
}

function setUp(win) {
  if (win.__shellmanWindow) return;
  if (win.document.documentElement.getAttribute("windowtype") !== "navigator:browser") return;
  win.__shellmanWindow = true;
  // After Windows has finished deactivating the window, not in the middle of it.
  win.addEventListener("deactivate", () => win.setTimeout(() => keepActiveLook(win), 0));
}

const observer = {
  observe(subject, topic) {
    if (topic === "browser-delayed-startup-finished") {
      try { setUp(subject); } catch (e) { logError("window glass", e); }
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
      try { setUp(win); } catch (e) { logError("window glass", e); }
    }
  }
}
