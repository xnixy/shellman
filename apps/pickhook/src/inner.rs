//! The work behind both dialog classes (see `dialog`). Each object owns a real Windows dialog
//! and passes every call on to it as the app makes it, noting what matters on the way. So when
//! `Show` comes, either the picker runs with what was noted, or the real dialog, which already
//! holds everything (types, options, custom controls, event sinks), is shown as if nothing had
//! happened. After a picker run, the getters answer from the picker's choice.

use crate::collected::{self, Collected};
use crate::items;
use crate::launch::{self, Hooks, Ran};
use shellman_pick::{FileType, Response};
use std::cell::{RefCell, RefMut};
use windows::core::{Interface, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, E_NOTIMPL, E_UNEXPECTED, HWND, S_FALSE};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    IFileDialog, IFileDialog2, IFileDialogCustomize, IFileDialogEvents, IFileOpenDialog,
    IFileSaveDialog, IShellItem, IShellItemArray, IShellItemFilter, FDAP,
    FILEOPENDIALOGOPTIONS,
};
use windows_core::{Error, Ref, Result};

/// Turn the hook off for this process: every dialog is the real one.
pub const OFF_VAR: &str = "SHELLMAN_PICKHOOK";

#[derive(Default)]
struct State {
    collected: Collected,
    guid: Option<String>,
    /// The app's event sinks, by the cookie the real dialog gave them.
    sinks: Vec<(u32, IFileDialogEvents)>,
    /// The picker's choice, once made (and while the app checks it).
    chosen: Option<Response>,
    /// The app asked the dialog to close (`Close`) during `Show`, with this code.
    closed: Option<HRESULT>,
    showing: bool,
}

pub struct Inner {
    save: bool,
    real: IFileDialog,
    state: RefCell<State>,
}

impl Inner {
    pub fn new(save: bool) -> Result<Self> {
        let real = crate::real::create(save)?;
        Ok(Self {
            save,
            real,
            state: RefCell::new(State {
                collected: Collected {
                    save,
                    ..Default::default()
                },
                ..Default::default()
            }),
        })
    }

    /// The state; never held across a call into the app or the real dialog, which may call
    /// back in.
    fn st(&self) -> RefMut<'_, State> {
        self.state.borrow_mut()
    }

    /// Note something the picker cannot do; the real dialog will be shown.
    pub fn unsupported(&self, why: &'static str) {
        let mut st = self.st();
        if !st.collected.unsupported.contains(&why) {
            st.collected.unsupported.push(why);
        }
    }

    pub fn custom(&self) -> Result<IFileDialogCustomize> {
        self.real.cast()
    }

    fn chosen(&self) -> Option<Response> {
        self.st().chosen.clone()
    }

    // ------------------------------------------------------------------ IModalWindow

    pub fn show(&self, owner: HWND, me: &IFileDialog) -> Result<()> {
        if std::mem::replace(&mut self.st().showing, true) {
            return Err(Error::from(E_UNEXPECTED));
        }
        let result = self.show_inner(owner, me);
        self.st().showing = false;
        result
    }

    fn show_inner(&self, owner: HWND, me: &IFileDialog) -> Result<()> {
        {
            let mut st = self.st();
            st.chosen = None;
            st.closed = None;
        }
        let options = unsafe { self.real.GetOptions() }.map_or(0, |o| o.0);
        let mut c = {
            let st = self.st();
            let mut c = st.collected.clone();
            c.client = collected::client(&host_exe(), st.guid.as_deref());
            c
        };
        c.options = options;
        let plan = if std::env::var_os(OFF_VAR).is_some_and(|v| v == "off") {
            Err("turned off for this process")
        } else {
            collected::request(&c)
        };
        let req = match plan {
            Ok(req) => req,
            Err(why) => return self.show_real(owner, why),
        };
        let has_sinks = !self.st().sinks.is_empty();
        let mut check = |resp: &Response| self.file_ok(resp, me);
        let closed = || self.state.borrow().closed.is_some();
        let hooks = Hooks {
            check: if has_sinks { Some(&mut check) } else { None },
            closed: &closed,
        };
        crate::log::line(&format!("picker for {} ({:?})", c.client, req.mode));
        match launch::run(req, owner, hooks) {
            Ran::Chosen(resp) => {
                crate::log::line(&format!("chosen: {:?}", resp.paths));
                self.st().chosen = Some(resp);
                Ok(())
            }
            Ran::Cancelled => {
                crate::log::line("cancelled");
                // After `Close(hr)`, `Show` returns that code, as the real dialog's does.
                match self.st().closed.unwrap_or(ERROR_CANCELLED.to_hresult()) {
                    hr if hr.is_ok() => Ok(()),
                    hr => Err(Error::from(hr)),
                }
            }
            Ran::Failed(why) => self.show_real(owner, &why),
        }
    }

    /// The real dialog, which holds everything the app set.
    fn show_real(&self, owner: HWND, why: &str) -> Result<()> {
        crate::log::line(&format!("real dialog: {why}"));
        let owner = (!owner.0.is_null()).then_some(owner);
        unsafe { self.real.Show(owner) }
    }

    /// Ask the app's sinks about a choice (`OnFileOk`), with the choice in place for the
    /// getters they call. True to accept it.
    fn file_ok(&self, resp: &Response, me: &IFileDialog) -> bool {
        self.st().chosen = Some(resp.clone());
        let sinks: Vec<IFileDialogEvents> = self.st().sinks.iter().map(|(_, s)| s.clone()).collect();
        for sink in sinks {
            // Called through the table, not the wrapper: the wrapper treats S_FALSE ("refuse")
            // as success.
            let hr = unsafe { (Interface::vtable(&sink).OnFileOk)(sink.as_raw(), me.as_raw()) };
            if refuses(hr) {
                crate::log::line(&format!("OnFileOk refused ({hr:?})"));
                self.st().chosen = None;
                return false;
            }
        }
        true
    }

    // ------------------------------------------------------------------ IFileDialog

    pub fn set_file_types(&self, count: u32, specs: *const COMDLG_FILTERSPEC) -> Result<()> {
        let list = if specs.is_null() {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(specs, count as usize) }
        };
        unsafe { self.real.SetFileTypes(list)? };
        self.st().collected.types = list
            .iter()
            .map(|s| FileType::parse(&items::text(&s.pszName), &items::text(&s.pszSpec)))
            .collect();
        Ok(())
    }

    pub fn set_file_type_index(&self, i: u32) -> Result<()> {
        unsafe { self.real.SetFileTypeIndex(i)? };
        self.st().collected.type_index = i;
        Ok(())
    }

    pub fn get_file_type_index(&self) -> Result<u32> {
        match self.chosen() {
            Some(r) => Ok(r.type_index as u32 + 1),
            None => unsafe { self.real.GetFileTypeIndex() },
        }
    }

    pub fn advise(&self, sink: Ref<'_, IFileDialogEvents>) -> Result<u32> {
        let sink = sink.ok()?.clone();
        let cookie = unsafe { self.real.Advise(&sink)? };
        self.st().sinks.push((cookie, sink));
        Ok(cookie)
    }

    pub fn unadvise(&self, cookie: u32) -> Result<()> {
        self.st().sinks.retain(|(c, _)| *c != cookie);
        unsafe { self.real.Unadvise(cookie) }
    }

    pub fn set_options(&self, fos: FILEOPENDIALOGOPTIONS) -> Result<()> {
        unsafe { self.real.SetOptions(fos) }
    }

    pub fn get_options(&self) -> Result<FILEOPENDIALOGOPTIONS> {
        unsafe { self.real.GetOptions() }
    }

    pub fn set_default_folder(&self, psi: Ref<'_, IShellItem>) -> Result<()> {
        unsafe { self.real.SetDefaultFolder(psi.as_ref())? };
        self.st().collected.default_folder = psi.as_ref().and_then(items::path_of);
        Ok(())
    }

    pub fn set_folder(&self, psi: Ref<'_, IShellItem>) -> Result<()> {
        unsafe { self.real.SetFolder(psi.as_ref())? };
        self.st().collected.folder = psi.as_ref().and_then(items::path_of);
        Ok(())
    }

    pub fn get_folder(&self) -> Result<IShellItem> {
        match self.chosen().and_then(|r| r.folder) {
            Some(f) => items::item(&f),
            None => unsafe { self.real.GetFolder() },
        }
    }

    pub fn get_current_selection(&self) -> Result<IShellItem> {
        match self.chosen().and_then(|r| r.paths.first().cloned()) {
            Some(p) => items::item(&p),
            None => unsafe { self.real.GetCurrentSelection() },
        }
    }

    pub fn set_file_name(&self, name: &PCWSTR) -> Result<()> {
        unsafe { self.real.SetFileName(*name)? };
        self.st().collected.file_name = items::text(name);
        Ok(())
    }

    pub fn get_file_name(&self) -> Result<PWSTR> {
        let chosen = self.chosen().and_then(|r| r.paths.first().cloned());
        match chosen {
            Some(p) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Ok(items::task_string(&name))
            }
            None => unsafe { self.real.GetFileName() },
        }
    }

    pub fn set_title(&self, title: &PCWSTR) -> Result<()> {
        unsafe { self.real.SetTitle(*title)? };
        self.st().collected.title = items::text(title);
        Ok(())
    }

    pub fn set_ok_button_label(&self, text: &PCWSTR) -> Result<()> {
        unsafe { self.real.SetOkButtonLabel(*text)? };
        self.st().collected.ok_label = items::text(text);
        Ok(())
    }

    pub fn set_file_name_label(&self, text: &PCWSTR) -> Result<()> {
        unsafe { self.real.SetFileNameLabel(*text)? };
        self.st().collected.file_name_label = items::text(text);
        Ok(())
    }

    pub fn get_result(&self) -> Result<IShellItem> {
        match self.chosen().and_then(|r| r.paths.first().cloned()) {
            Some(p) => items::item(&p),
            None => unsafe { self.real.GetResult() },
        }
    }

    pub fn add_place(&self, psi: Ref<'_, IShellItem>, fdap: FDAP) -> Result<()> {
        unsafe { self.real.AddPlace(psi.as_ref(), fdap) }
    }

    pub fn set_default_extension(&self, ext: &PCWSTR) -> Result<()> {
        unsafe { self.real.SetDefaultExtension(*ext)? };
        self.st().collected.default_ext = items::text(ext);
        Ok(())
    }

    pub fn close(&self, hr: HRESULT) -> Result<()> {
        self.st().closed = Some(hr);
        unsafe { self.real.Close(hr) }
    }

    pub fn set_client_guid(&self, guid: *const GUID) -> Result<()> {
        unsafe { self.real.SetClientGuid(guid)? };
        if !guid.is_null() {
            self.st().guid = Some(format!("{:?}", unsafe { *guid }));
        }
        Ok(())
    }

    pub fn clear_client_data(&self) -> Result<()> {
        unsafe { self.real.ClearClientData() }
    }

    pub fn set_filter(&self, filter: Ref<'_, IShellItemFilter>) -> Result<()> {
        if filter.is_some() {
            self.unsupported("an item filter");
        }
        unsafe { self.real.SetFilter(filter.as_ref()) }
    }

    // ------------------------------------------------------------------ IFileDialog2

    fn real2(&self) -> Result<IFileDialog2> {
        self.real.cast()
    }

    pub fn set_cancel_button_label(&self, text: &PCWSTR) -> Result<()> {
        unsafe { self.real2()?.SetCancelButtonLabel(*text) }
    }

    pub fn set_navigation_root(&self, psi: Ref<'_, IShellItem>) -> Result<()> {
        if psi.is_some() {
            self.unsupported("a navigation root");
        }
        unsafe { self.real2()?.SetNavigationRoot(psi.as_ref()) }
    }

    // ------------------------------------------------------------------ IFileOpenDialog

    pub fn get_results(&self) -> Result<IShellItemArray> {
        match self.chosen() {
            Some(r) if !self.save => items::array(&r.paths),
            _ => unsafe { self.real.cast::<IFileOpenDialog>()?.GetResults() },
        }
    }

    pub fn get_selected_items(&self) -> Result<IShellItemArray> {
        match self.chosen() {
            Some(r) if !self.save => items::array(&r.paths),
            _ => unsafe { self.real.cast::<IFileOpenDialog>()?.GetSelectedItems() },
        }
    }

    // ------------------------------------------------------------------ IFileSaveDialog

    pub fn real_save(&self) -> Result<IFileSaveDialog> {
        self.real.cast()
    }

    /// The item to save as: its folder and name start the picker.
    pub fn set_save_as_item(&self, psi: Ref<'_, IShellItem>) -> Result<()> {
        unsafe { self.real_save()?.SetSaveAsItem(psi.as_ref())? };
        if let Some(p) = psi.as_ref().and_then(items::path_of) {
            let mut st = self.st();
            st.collected.folder = p.parent().map(Into::into);
            st.collected.file_name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
        Ok(())
    }
}

/// Whether a sink's answer to `OnFileOk` refuses the choice. S_FALSE does; so does a failure,
/// except "not implemented", which sinks that ignore the event return.
fn refuses(hr: HRESULT) -> bool {
    hr == S_FALSE || (hr.is_err() && hr != E_NOTIMPL)
}

/// The program this DLL is loaded into, as a full path.
fn host_exe() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{E_FAIL, S_OK};

    #[test]
    fn s_false_and_failures_refuse_but_not_implemented_does_not() {
        assert!(!refuses(S_OK));
        assert!(refuses(S_FALSE));
        assert!(refuses(E_FAIL));
        assert!(!refuses(E_NOTIMPL));
    }
}
