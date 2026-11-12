//! The two COM classes the factory makes: an open dialog and a save dialog. They differ only in
//! their own interface (`IFileOpenDialog` or `IFileSaveDialog`); everything they share is
//! written once below, as a macro over the two, and does its work in `Inner`. Every method runs
//! through `guard`, so a panic is an error code, never a crash in the app.

use crate::guard::guard;
use crate::inner::Inner;
use windows::core::{implement, Interface, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, E_NOTIMPL};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyDescriptionList, IPropertyStore};
use windows::Win32::UI::Shell::{
    IFileDialog, IFileDialog2, IFileDialog2_Impl, IFileDialogCustomize, IFileDialogCustomize_Impl,
    IFileDialogEvents, IFileDialog_Impl, IFileOpenDialog, IFileOpenDialog_Impl,
    IFileOperationProgressSink, IFileSaveDialog, IFileSaveDialog_Impl, IModalWindow_Impl,
    IShellItem, IShellItemArray, IShellItemFilter, CDCONTROLSTATEF, FDAP, FILEOPENDIALOGOPTIONS,
};
use windows::Win32::System::Ole::{IOleWindow, IOleWindow_Impl};
use windows_core::{IUnknownImpl, Ref, Result, BOOL};

/// What an app gets for `CLSID_FileOpenDialog`.
#[implement(IFileOpenDialog, IFileDialog2, IFileDialogCustomize, IOleWindow, Agile = false)]
pub struct OpenDialog {
    inner: Inner,
}

/// What an app gets for `CLSID_FileSaveDialog`.
#[implement(IFileSaveDialog, IFileDialog2, IFileDialogCustomize, IOleWindow, Agile = false)]
pub struct SaveDialog {
    inner: Inner,
}

impl OpenDialog {
    pub fn new() -> Result<Self> {
        Ok(Self { inner: Inner::new(false)? })
    }
}

impl SaveDialog {
    pub fn new() -> Result<Self> {
        Ok(Self { inner: Inner::new(true)? })
    }
}

impl IFileOpenDialog_Impl for OpenDialog_Impl {
    fn GetResults(&self) -> Result<IShellItemArray> {
        guard(|| crate::log::trace("GetResults", self.inner.get_results()))
    }
    fn GetSelectedItems(&self) -> Result<IShellItemArray> {
        guard(|| crate::log::trace("GetSelectedItems", self.inner.get_selected_items()))
    }
}

impl IFileSaveDialog_Impl for SaveDialog_Impl {
    fn SetSaveAsItem(&self, psi: Ref<'_, IShellItem>) -> Result<()> {
        guard(|| self.inner.set_save_as_item(psi))
    }
    fn SetProperties(&self, store: Ref<'_, IPropertyStore>) -> Result<()> {
        guard(|| unsafe { self.inner.real_save()?.SetProperties(store.as_ref()) })
    }
    fn SetCollectedProperties(&self, list: Ref<'_, IPropertyDescriptionList>, append: BOOL) -> Result<()> {
        guard(|| {
            // The real dialog shows these properties for editing; the picker has no place for them.
            self.inner.unsupported("properties to collect");
            unsafe { self.inner.real_save()?.SetCollectedProperties(list.as_ref(), append.as_bool()) }
        })
    }
    fn GetProperties(&self) -> Result<IPropertyStore> {
        guard(|| unsafe { self.inner.real_save()?.GetProperties() })
    }
    fn ApplyProperties(
        &self,
        psi: Ref<'_, IShellItem>,
        store: Ref<'_, IPropertyStore>,
        hwnd: HWND,
        sink: Ref<'_, IFileOperationProgressSink>,
    ) -> Result<()> {
        guard(|| unsafe {
            self.inner.real_save()?.ApplyProperties(psi.as_ref(), store.as_ref(), hwnd, sink.as_ref())
        })
    }
}

/// The interfaces both classes share, for `$t` (the class's `_Impl` type) whose own interface
/// is `$own`.
macro_rules! common {
    ($t:ty, $own:ty) => {
        impl IModalWindow_Impl for $t {
            fn Show(&self, owner: HWND) -> Result<()> {
                guard(|| {
                    // This object as the app knows it, for the events' `pfd`.
                    let me: IFileDialog = self.to_object().to_interface::<$own>().cast()?;
                    self.inner.show(owner, &me)
                })
            }
        }

        /// The real dialog has a window and says so; apps ask for it from their events.
        impl IOleWindow_Impl for $t {
            fn GetWindow(&self) -> Result<HWND> {
                guard(|| {
                    crate::log::trace(
                        "GetWindow",
                        crate::launch::picker_window().ok_or_else(|| windows::Win32::Foundation::E_FAIL.into()),
                    )
                })
            }
            fn ContextSensitiveHelp(&self, _enter: BOOL) -> Result<()> {
                Err(E_NOTIMPL.into())
            }
        }

        impl IFileDialog_Impl for $t {
            fn SetFileTypes(&self, n: u32, specs: *const COMDLG_FILTERSPEC) -> Result<()> {
                guard(|| self.inner.set_file_types(n, specs))
            }
            fn SetFileTypeIndex(&self, i: u32) -> Result<()> {
                guard(|| self.inner.set_file_type_index(i))
            }
            fn GetFileTypeIndex(&self) -> Result<u32> {
                guard(|| crate::log::trace("GetFileTypeIndex", self.inner.get_file_type_index()))
            }
            fn Advise(&self, sink: Ref<'_, IFileDialogEvents>) -> Result<u32> {
                guard(|| self.inner.advise(sink))
            }
            fn Unadvise(&self, cookie: u32) -> Result<()> {
                guard(|| self.inner.unadvise(cookie))
            }
            fn SetOptions(&self, fos: FILEOPENDIALOGOPTIONS) -> Result<()> {
                guard(|| self.inner.set_options(fos))
            }
            fn GetOptions(&self) -> Result<FILEOPENDIALOGOPTIONS> {
                guard(|| crate::log::trace("GetOptions", self.inner.get_options()))
            }
            fn SetDefaultFolder(&self, psi: Ref<'_, IShellItem>) -> Result<()> {
                guard(|| self.inner.set_default_folder(psi))
            }
            fn SetFolder(&self, psi: Ref<'_, IShellItem>) -> Result<()> {
                guard(|| self.inner.set_folder(psi))
            }
            fn GetFolder(&self) -> Result<IShellItem> {
                guard(|| crate::log::trace("GetFolder", self.inner.get_folder()))
            }
            fn GetCurrentSelection(&self) -> Result<IShellItem> {
                guard(|| crate::log::trace("GetCurrentSelection", self.inner.get_current_selection()))
            }
            fn SetFileName(&self, name: &PCWSTR) -> Result<()> {
                guard(|| self.inner.set_file_name(name))
            }
            fn GetFileName(&self) -> Result<PWSTR> {
                guard(|| crate::log::trace("GetFileName", self.inner.get_file_name()))
            }
            fn SetTitle(&self, title: &PCWSTR) -> Result<()> {
                guard(|| self.inner.set_title(title))
            }
            fn SetOkButtonLabel(&self, text: &PCWSTR) -> Result<()> {
                guard(|| self.inner.set_ok_button_label(text))
            }
            fn SetFileNameLabel(&self, text: &PCWSTR) -> Result<()> {
                guard(|| self.inner.set_file_name_label(text))
            }
            fn GetResult(&self) -> Result<IShellItem> {
                guard(|| crate::log::trace("GetResult", self.inner.get_result()))
            }
            fn AddPlace(&self, psi: Ref<'_, IShellItem>, fdap: FDAP) -> Result<()> {
                guard(|| self.inner.add_place(psi, fdap))
            }
            fn SetDefaultExtension(&self, ext: &PCWSTR) -> Result<()> {
                guard(|| self.inner.set_default_extension(ext))
            }
            fn Close(&self, hr: HRESULT) -> Result<()> {
                guard(|| self.inner.close(hr))
            }
            fn SetClientGuid(&self, guid: *const GUID) -> Result<()> {
                guard(|| self.inner.set_client_guid(guid))
            }
            fn ClearClientData(&self) -> Result<()> {
                guard(|| self.inner.clear_client_data())
            }
            fn SetFilter(&self, filter: Ref<'_, IShellItemFilter>) -> Result<()> {
                guard(|| self.inner.set_filter(filter))
            }
        }

        impl IFileDialog2_Impl for $t {
            fn SetCancelButtonLabel(&self, text: &PCWSTR) -> Result<()> {
                guard(|| self.inner.set_cancel_button_label(text))
            }
            fn SetNavigationRoot(&self, psi: Ref<'_, IShellItem>) -> Result<()> {
                guard(|| self.inner.set_navigation_root(psi))
            }
        }

        /// Custom controls go straight to the real dialog, which keeps and shows them; adding
        /// any means the real dialog is shown, since the picker cannot.
        impl IFileDialogCustomize_Impl for $t {
            fn EnableOpenDropDown(&self, id: u32) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.EnableOpenDropDown(id) }
                })
            }
            fn AddMenu(&self, id: u32, label: &PCWSTR) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.AddMenu(id, *label) }
                })
            }
            fn AddPushButton(&self, id: u32, label: &PCWSTR) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.AddPushButton(id, *label) }
                })
            }
            fn AddComboBox(&self, id: u32) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.AddComboBox(id) }
                })
            }
            fn AddRadioButtonList(&self, id: u32) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.AddRadioButtonList(id) }
                })
            }
            fn AddCheckButton(&self, id: u32, label: &PCWSTR, checked: BOOL) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.AddCheckButton(id, *label, checked.as_bool()) }
                })
            }
            fn AddEditBox(&self, id: u32, text: &PCWSTR) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.AddEditBox(id, *text) }
                })
            }
            fn AddSeparator(&self, id: u32) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.AddSeparator(id) }
                })
            }
            fn AddText(&self, id: u32, text: &PCWSTR) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.AddText(id, *text) }
                })
            }
            fn SetControlLabel(&self, id: u32, label: &PCWSTR) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.SetControlLabel(id, *label) })
            }
            fn GetControlState(&self, id: u32) -> Result<CDCONTROLSTATEF> {
                guard(|| unsafe { self.inner.custom()?.GetControlState(id) })
            }
            fn SetControlState(&self, id: u32, state: CDCONTROLSTATEF) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.SetControlState(id, state) })
            }
            fn GetEditBoxText(&self, id: u32) -> Result<*mut u16> {
                guard(|| unsafe { self.inner.custom()?.GetEditBoxText(id) })
            }
            fn SetEditBoxText(&self, id: u32, text: &PCWSTR) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.SetEditBoxText(id, *text) })
            }
            fn GetCheckButtonState(&self, id: u32) -> Result<BOOL> {
                guard(|| unsafe { self.inner.custom()?.GetCheckButtonState(id) })
            }
            fn SetCheckButtonState(&self, id: u32, checked: BOOL) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.SetCheckButtonState(id, checked.as_bool()) })
            }
            fn AddControlItem(&self, id: u32, item: u32, label: &PCWSTR) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.AddControlItem(id, item, *label) })
            }
            fn RemoveControlItem(&self, id: u32, item: u32) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.RemoveControlItem(id, item) })
            }
            fn RemoveAllControlItems(&self, _id: u32) -> Result<()> {
                // Documented as not implemented by Windows itself.
                Err(E_NOTIMPL.into())
            }
            fn GetControlItemState(&self, id: u32, item: u32) -> Result<CDCONTROLSTATEF> {
                guard(|| unsafe { self.inner.custom()?.GetControlItemState(id, item) })
            }
            fn SetControlItemState(&self, id: u32, item: u32, state: CDCONTROLSTATEF) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.SetControlItemState(id, item, state) })
            }
            fn GetSelectedControlItem(&self, id: u32) -> Result<u32> {
                guard(|| unsafe { self.inner.custom()?.GetSelectedControlItem(id) })
            }
            fn SetSelectedControlItem(&self, id: u32, item: u32) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.SetSelectedControlItem(id, item) })
            }
            fn StartVisualGroup(&self, id: u32, label: &PCWSTR) -> Result<()> {
                guard(|| {
                    self.inner.unsupported("custom controls");
                    unsafe { self.inner.custom()?.StartVisualGroup(id, *label) }
                })
            }
            fn EndVisualGroup(&self) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.EndVisualGroup() })
            }
            fn MakeProminent(&self, id: u32) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.MakeProminent(id) })
            }
            fn SetControlItemText(&self, id: u32, item: u32, label: &PCWSTR) -> Result<()> {
                guard(|| unsafe { self.inner.custom()?.SetControlItemText(id, item, *label) })
            }
        }
    };
}

common!(OpenDialog_Impl, IFileOpenDialog);
common!(SaveDialog_Impl, IFileSaveDialog);
