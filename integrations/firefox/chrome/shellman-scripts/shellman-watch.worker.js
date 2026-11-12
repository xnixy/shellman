/* shellman Islands: waits for changes in one folder (the one holding appearance.json) and tells the
 * browser. Runs in its own thread (ChromeWorker) so the wait never blocks Firefox.
 *
 * FindFirstChangeNotificationW + WaitForMultipleObjects: the thread sleeps in the kernel until the
 * folder changes or the browser signals `stop`. The wait also returns every SAFETY_MS so the worker
 * yields to its event loop and can be terminated even if the stop signal were lost; that wakeup does
 * no file I/O. */
"use strict";

const SAFETY_MS = 10000;
const FILE_NOTIFY_CHANGE_FILE_NAME = 0x1;
const FILE_NOTIFY_CHANGE_LAST_WRITE = 0x10;
const WAIT_OBJECT_0 = 0;
const WAIT_TIMEOUT = 0x102;
const INVALID = -1;

let k32, FindFirst, FindNext, FindCloseN, WaitMulti, handles, change;

function start(dir, stopValue) {
  k32 = ctypes.open("kernel32.dll");
  const HANDLE = ctypes.voidptr_t;
  FindFirst = k32.declare("FindFirstChangeNotificationW", ctypes.winapi_abi, HANDLE, ctypes.char16_t.ptr, ctypes.int32_t, ctypes.uint32_t);
  FindNext = k32.declare("FindNextChangeNotification", ctypes.winapi_abi, ctypes.int32_t, HANDLE);
  FindCloseN = k32.declare("FindCloseChangeNotification", ctypes.winapi_abi, ctypes.int32_t, HANDLE);
  WaitMulti = k32.declare("WaitForMultipleObjects", ctypes.winapi_abi, ctypes.uint32_t, ctypes.uint32_t, HANDLE.ptr, ctypes.int32_t, ctypes.uint32_t);

  change = FindFirst(dir, 0, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE);
  if (ctypes.cast(change, ctypes.intptr_t).value.toString() === String(INVALID)) {
    postMessage({ type: "error", message: "FindFirstChangeNotificationW failed for " + dir + " (" + ctypes.winLastError + ")" });
    close();
    return;
  }
  const stop = ctypes.cast(ctypes.uintptr_t(ctypes.UInt64(stopValue)), HANDLE);
  handles = HANDLE.array(2)([stop, change]);
  postMessage({ type: "ready" });
  wait();
}

function wait() {
  const r = WaitMulti(2, handles, 0, SAFETY_MS);
  if (r === WAIT_OBJECT_0 + 1) {
    postMessage({ type: "changed" });
    if (!FindNext(change)) {
      postMessage({ type: "error", message: "FindNextChangeNotification failed (" + ctypes.winLastError + ")" });
      return finish();
    }
  } else if (r !== WAIT_TIMEOUT) {
    return finish(); // stop signalled (WAIT_OBJECT_0) or the wait failed
  }
  setTimeout(wait, 0); // back to the event loop between waits, so terminate() always gets through
}

function finish() {
  try { FindCloseN(change); } catch (_) {}
  try { k32.close(); } catch (_) {}
  postMessage({ type: "stopped" });
  close();
}

onmessage = (m) => {
  try {
    start(m.data.dir, m.data.stop);
  } catch (e) {
    postMessage({ type: "error", message: String(e) });
    close();
  }
};
