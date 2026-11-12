//! Reading the shell's menu into `Item`s. The handlers fill a hidden menu that is never shown;
//! submenus they fill only when opened get the message a shown menu would send them first.

use super::model::{parse_label, Cascade, Item};
use super::paint::{self, Canvas};
use super::slow::Plan;
use std::time::{Duration, Instant};
use windows::core::{Interface, PSTR, PWSTR};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{IContextMenu, IContextMenu2, IContextMenu3};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemInfoW, HBMMENU_CALLBACK, HMENU,
    MENUITEMINFOW, MFS_CHECKED, MFS_DEFAULT, MFS_DISABLED, MFT_OWNERDRAW, MFT_SEPARATOR,
    MIIM_BITMAP, MIIM_CHECKMARKS, MIIM_DATA, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING,
    MIIM_SUBMENU,
};

// QueryContextMenu flags: CMF_EXPLORE 0x4 (as Explorer's own window asks), CMF_CANRENAME 0x10,
// CMF_EXTENDEDVERBS 0x100 (the extra entries Explorer shows with Shift held).
const CMF_EXPLORE: u32 = 0x4;
const CMF_CANRENAME: u32 = 0x10;
const CMF_EXTENDEDVERBS: u32 = 0x100;
pub(super) const FIRST_ID: u32 = 1;
const LAST_ID: u32 = 0x7FFF;
const GCS_VERBW: u32 = 4;
const GCS_HELPTEXTW: u32 = 5;
const WM_INITMENUPOPUP: u32 = 0x0117;

/// A submenu Windows has not been asked to fill yet: where it is in the tree, and its handle.
struct Open {
    path: Vec<usize>,
    menu: HMENU,
    /// Its position in the parent menu, which WM_INITMENUPOPUP carries.
    pos: u32,
    key: String,
}

/// The shell's menu for one target, kept alive (with its handlers) until a command has run or
/// it is no longer wanted.
pub(super) struct Walk {
    pub(super) cm: IContextMenu,
    cm2: Option<IContextMenu2>,
    cm3: Option<IContextMenu3>,
    hmenu: HMENU,
    canvas: Option<Canvas>,
    /// Submenus still to fill, in the order they will be.
    queue: Vec<Open>,
    /// Submenus left for the person to open, because their handler was too slow before.
    deferred: Vec<Open>,
}

impl Walk {
    /// Has the handlers fill a hidden menu. This is where every extension loads, and where a
    /// slow one costs time.
    pub(super) fn query(cm: IContextMenu, extended: bool, palette: paint::Palette) -> Option<Self> {
        unsafe {
            let hmenu = CreatePopupMenu().ok()?;
            let mut flags = CMF_EXPLORE | CMF_CANRENAME;
            if extended {
                flags |= CMF_EXTENDEDVERBS;
            }
            if cm.QueryContextMenu(hmenu, 0, FIRST_ID, LAST_ID, flags).is_err() {
                let _ = DestroyMenu(hmenu);
                return None;
            }
            Some(Self {
                cm2: cm.cast().ok(),
                cm3: cm.cast().ok(),
                cm,
                hmenu,
                canvas: Canvas::new(palette),
                queue: Vec::new(),
                deferred: Vec::new(),
            })
        }
    }

    /// The top level, with submenus that are already filled read at once and the rest queued.
    pub(super) fn top(&mut self, plan: &Plan) -> Vec<Item> {
        let mut found = Vec::new();
        let mut items = self.read(self.hmenu, &[], &mut found);
        self.enqueue(found, plan, &mut items);
        items
    }

    /// Fills the next queued submenu and reads it into `items`. Returns what it was and how long
    /// it took, or None when nothing is left.
    pub(super) fn fill_next(&mut self, items: &mut [Item], plan: &Plan) -> Option<(String, Duration)> {
        if self.queue.is_empty() {
            return None;
        }
        let open = self.queue.remove(0);
        Some(self.fill(open, items, plan))
    }

    /// Fills a submenu that was deferred, now that the person opened it.
    pub(super) fn fill_deferred(&mut self, path: &[usize], items: &mut [Item], plan: &Plan) -> Option<(String, Duration)> {
        let i = self.deferred.iter().position(|o| o.path == path)?;
        let open = self.deferred.remove(i);
        Some(self.fill(open, items, plan))
    }

    fn fill(&mut self, open: Open, items: &mut [Item], plan: &Plan) -> (String, Duration) {
        let started = Instant::now();
        // As a shown menu would: WM_INITMENUPOPUP with the submenu and its position.
        self.send(WM_INITMENUPOPUP, WPARAM(open.menu.0 as usize), LPARAM(open.pos as isize));
        let mut found = Vec::new();
        let read = self.read(open.menu, &open.path, &mut found);
        if let Some(item) = item_at(items, &open.path) {
            item.submenu = read;
            item.cascade = Cascade::Filled;
        }
        self.enqueue(found, plan, items);
        (open.key, started.elapsed())
    }

    /// Queues found submenus: known-slow ones last, ones that hung before left for later.
    fn enqueue(&mut self, found: Vec<Open>, plan: &Plan, items: &mut [Item]) {
        for open in found {
            match plan.of(&open.key) {
                super::slow::Treat::Normal => {
                    let at = self
                        .queue
                        .iter()
                        .position(|o| plan.of(&o.key) != super::slow::Treat::Normal)
                        .unwrap_or(self.queue.len());
                    self.queue.insert(at, open);
                }
                super::slow::Treat::Last => self.queue.push(open),
                super::slow::Treat::Skip => {
                    if let Some(item) = item_at(items, &open.path) {
                        item.cascade = Cascade::Deferred;
                    }
                    self.deferred.push(open);
                }
            }
        }
    }

    /// What the next queued submenu is, for timing it from outside.
    pub(super) fn next_key(&self) -> Option<String> {
        self.queue.first().map(|o| o.key.clone())
    }

    /// The submenus still queued (not counting deferred ones).
    pub(super) fn pending(&self) -> usize {
        self.queue.len()
    }

    /// One menu level. Submenus are noted in `found` to be filled, and marked as pending.
    fn read(&self, menu: HMENU, path: &[usize], found: &mut Vec<Open>) -> Vec<Item> {
        let n = unsafe { GetMenuItemCount(Some(menu)) }.max(0) as u32;
        let mut items = Vec::with_capacity(n as usize);
        for pos in 0..n {
            let Some((mut item, sub, data)) = self.read_item(menu, pos) else {
                continue;
            };
            if item.owner_drawn {
                // A row the extension draws itself: render it, so it shows as it was meant to.
                item.drawn = self.draw(menu, item.id + FIRST_ID, data);
            }
            let mut child_path = path.to_vec();
            child_path.push(items.len());
            item.path = child_path.clone();
            if let Some(sub) = sub {
                item.cascade = Cascade::Pending;
                found.push(Open { path: child_path, menu: sub, pos, key: item.slow_key() });
            }
            items.push(item);
        }
        items
    }

    /// One entry: what it says and shows, its submenu handle if any, and its item data (which
    /// owner-drawn handlers need back).
    fn read_item(&self, menu: HMENU, pos: u32) -> Option<(Item, Option<HMENU>, usize)> {
        let mut mii = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE
                | MIIM_STATE
                | MIIM_ID
                | MIIM_SUBMENU
                | MIIM_STRING
                | MIIM_BITMAP
                | MIIM_CHECKMARKS
                | MIIM_DATA,
            ..Default::default()
        };
        unsafe { GetMenuItemInfoW(menu, pos, true, &mut mii) }.ok()?;
        let separator = mii.fType.0 & MFT_SEPARATOR.0 != 0;
        if separator {
            return Some((Item::separator(), None, 0));
        }
        let mut text = String::new();
        if mii.cch > 0 {
            // A second, text-only query of its own: the first one's fields stay as they were.
            let mut buf = vec![0u16; mii.cch as usize + 1];
            let mut t = MENUITEMINFOW {
                cbSize: size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_STRING,
                dwTypeData: PWSTR(buf.as_mut_ptr()),
                cch: buf.len() as u32,
                ..Default::default()
            };
            if unsafe { GetMenuItemInfoW(menu, pos, true, &mut t) }.is_ok() {
                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                text = String::from_utf16_lossy(&buf[..end]);
            }
        }
        let (label, mnemonic, shortcut) = parse_label(&text);
        let has_sub = !mii.hSubMenu.is_invalid();
        let owner_drawn = mii.fType.0 & MFT_OWNERDRAW.0 != 0;
        let id = mii.wID.wrapping_sub(FIRST_ID);
        let in_range = mii.wID >= FIRST_ID && mii.wID <= LAST_ID;
        let verb = (!has_sub && in_range).then(|| self.string(id, GCS_VERBW)).flatten();
        let help = (owner_drawn && label.is_empty() && in_range)
            .then(|| self.string(id, GCS_HELPTEXTW))
            .flatten();
        let mut icon = paint::bitmap(mii.hbmpItem).or_else(|| paint::bitmap(mii.hbmpUnchecked));        if icon.is_none() && mii.hbmpItem == HBMMENU_CALLBACK {
            // The icon is drawn on request, like an owner-drawn row.
            icon = self.draw(menu, mii.wID, mii.dwItemData);
        }
        let item = Item {
            id,
            label,
            mnemonic,
            shortcut,
            verb: verb.map(|v| v.to_lowercase()),
            help,
            icon,
            drawn: None,
            owner_drawn,
            enabled: mii.fState.0 & MFS_DISABLED.0 == 0,
            checked: mii.fState.0 & MFS_CHECKED.0 != 0,
            separator: false,
            default: mii.fState.0 & MFS_DEFAULT.0 != 0,
            cascade: Cascade::None,
            submenu: Vec::new(),
            path: Vec::new(),
        };
        Some((item, has_sub.then_some(mii.hSubMenu), mii.dwItemData))
    }

    fn draw(&self, menu: HMENU, cmd: u32, data: usize) -> Option<super::model::Icon> {
        let canvas = self.canvas.as_ref()?;
        canvas.render(&|m, w, l| self.send(m, w, l), menu, cmd, data)
    }

    /// A command's verb or help text.
    fn string(&self, id: u32, kind: u32) -> Option<String> {
        let mut buf = [0u16; 260];
        unsafe {
            self.cm
                .GetCommandString(
                    id as usize,
                    kind,
                    None,
                    PSTR(buf.as_mut_ptr().cast()),
                    buf.len() as u32,
                )
                .ok()?;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// Passes a menu message to the handlers, as a shown menu's owner window would.
    pub(super) fn send(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let mut result = LRESULT(0);
        unsafe {
            if let Some(cm3) = &self.cm3 {
                cm3.HandleMenuMsg2(msg, wparam, lparam, Some(&mut result)).ok()?;
            } else {
                self.cm2.as_ref()?.HandleMenuMsg(msg, wparam, lparam).ok()?;
            }
        }
        Some(result)
    }
}

impl Drop for Walk {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.hmenu);
        }
    }
}

/// The entry at `path`, to change it.
fn item_at<'a>(items: &'a mut [Item], path: &[usize]) -> Option<&'a mut Item> {
    let (&first, rest) = path.split_first()?;
    let item = items.get_mut(first)?;
    if rest.is_empty() {
        Some(item)
    } else {
        item_at(&mut item.submenu, rest)
    }
}
