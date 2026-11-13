//! Bindings for `Windows.Management.Update` (the Update Orchestrator's WinRT API), generated,
//! not written by hand. The `windows` crate 0.61 has only the first contract; the second one
//! (`IWindowsUpdateManager2`, `WindowsSoftwareUpdate`: sizes, categories, `Approve`,
//! `PerformScan`) came with Windows 11 26100.9168 and is read from that build's metadata. A
//! class method of the second contract casts at the call, so on an older build it fails with
//! E_NOINTERFACE instead of crashing. Do not edit; to regenerate (`windows-bindgen` 0.62.1):
//!
//! ```text
//! windows-bindgen --in default C:\Windows\System32\WinMetadata\Windows.Management.winmd
//!   --out wu_api.rs --flat --no-comment --reference windows,skip-root,Windows.Foundation
//!   --filter Windows.Management.Update.{WindowsUpdateManager,IWindowsUpdateManager2,WindowsUpdate,WindowsUpdateActionProgress,WindowsUpdateActionResult,WindowsUpdateAttentionRequiredInfo,WindowsUpdateAttentionRequiredReason,WindowsUpdateItem,WindowsUpdateManagerScanMode,WindowsUpdateActionCompletedEventArgs,WindowsUpdateProgressChangedEventArgs,WindowsUpdateAttentionRequiredReasonChangedEventArgs,WindowsUpdateScanCompletedEventArgs,WindowsSoftwareUpdate,WindowsSoftwareUpdateApprovalInfo,WindowsSoftwareUpdateActionProgress,WindowsSoftwareUpdateActionResultInfo,WindowsSoftwareUpdateActionType,WindowsSoftwareUpdateActionResult,WindowsSoftwareUpdateCategory,WindowsSoftwareUpdateRestartReason,WindowsSoftwareUpdateResult,WindowsSoftwareUpdateScanResult,WindowsSoftwareUpdateIdentity,WindowsSoftwareUpdateIdentityType}
//! ```
//! (each filter name in full, space-separated), then put this header back. That version is not
//! deterministic here: about one run in two leaves the second contract's class methods out (and
//! the newest reasons, 33 to 38, depend on which metadata wins). Keep a run whose output has
//! `GetApplicableSoftwareUpdates`, `PerformScan` and `Undefined = 38`.

#![allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    clippy::all
)]

windows_core::imp::define_interface!(
    IWindowsSoftwareUpdate,
    IWindowsSoftwareUpdate_Vtbl,
    0x7bcd30be_0a5e_540f_9bd8_b8b665980b8b
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdate {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdate_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    InstallationType: usize,
    pub ProviderId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub UpdateId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Title: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Description: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub MoreInfoUrl: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub IsSeeker:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub UpdateCategory: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut WindowsSoftwareUpdateCategory,
    ) -> windows_core::HRESULT,
    pub UpdateIdentity: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub DownloadSizeInBytes:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
    pub InstallSizeInBytes:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
    SourceVersion: usize,
    TargetVersion: usize,
    pub Approve: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ApproveCurrentAction: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        bool,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CurrentAction: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ActionResultInfo: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ApprovalInfo: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ApprovedActions: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub AttentionRequiredInfo: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ActionProgress: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub RestartReason: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    AppPackageInfo: usize,
    ExecutionInfo: usize,
    OptionalInfo: usize,
    pub Properties: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetPropertyValue: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateActionProgress,
    IWindowsSoftwareUpdateActionProgress_Vtbl,
    0x17dc15fd_75f2_522b_b555_359da8de5581
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateActionProgress {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateActionProgress_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Action: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CurrentProgress:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
    pub TotalProgress:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateActionResultInfo,
    IWindowsSoftwareUpdateActionResultInfo_Vtbl,
    0xbcf26fba_98c8_51cb_8b7e_1eedc3d82a69
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateActionResultInfo {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateActionResultInfo_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Timestamp: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows::Foundation::DateTime,
    ) -> windows_core::HRESULT,
    pub Succeeded:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub ResultCode:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32) -> windows_core::HRESULT,
    pub ExtendedError:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
    pub Action: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateApprovalInfo,
    IWindowsSoftwareUpdateApprovalInfo_Vtbl,
    0x691e6b9e_80af_5882_9404_25437ecb791b
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateApprovalInfo {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateApprovalInfo_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub UserInitiated:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub AppClosure:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub MeteredNetwork:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub Seeker:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateApprovalInfoFactory,
    IWindowsSoftwareUpdateApprovalInfoFactory_Vtbl,
    0xab291c7c_d29f_5ac5_b447_0bfcabdc2cc3
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateApprovalInfoFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateApprovalInfoFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        bool,
        bool,
        bool,
        bool,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateFactory,
    IWindowsSoftwareUpdateFactory_Vtbl,
    0x2a6e4e2d_3627_5943_b0ad_bf0b06ceae4c
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    CreateInstance: usize,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateIdentity,
    IWindowsSoftwareUpdateIdentity_Vtbl,
    0x6040c757_5447_5c1f_8b25_8976bce12786
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateIdentity {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateIdentity_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Type: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut WindowsSoftwareUpdateIdentityType,
    ) -> windows_core::HRESULT,
    pub Identity: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateIdentityFactory,
    IWindowsSoftwareUpdateIdentityFactory_Vtbl,
    0xc84999fc_8ce6_5711_b88c_d5b926e2e5db
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateIdentityFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateIdentityFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        WindowsSoftwareUpdateIdentityType,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateResult,
    IWindowsSoftwareUpdateResult_Vtbl,
    0xa6d7ed98_6212_5ad3_aa9d_15e83bb85ee4
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateResult_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Succeeded:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub CancelRequested:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub ResultCode:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32) -> windows_core::HRESULT,
    pub ExtendedError:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateResultFactory,
    IWindowsSoftwareUpdateResultFactory_Vtbl,
    0x512ce0bf_9977_5301_9b29_9e5042c8cf7d
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateResultFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateResultFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        bool,
        u32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateInstance2: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        bool,
        u32,
        u64,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateInstance3: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        bool,
        bool,
        u32,
        u64,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateScanResult,
    IWindowsSoftwareUpdateScanResult_Vtbl,
    0x43ca6d96_3e6d_56da_a903_65d4ab729299
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateScanResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateScanResult_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Succeeded:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub ResultCode:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32) -> windows_core::HRESULT,
    pub ExtendedError:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
    pub Updates: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsSoftwareUpdateScanResultFactory,
    IWindowsSoftwareUpdateScanResultFactory_Vtbl,
    0x21148e4c_e7ce_574e_bfa7_69dc77457d21
);
impl windows_core::RuntimeType for IWindowsSoftwareUpdateScanResultFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsSoftwareUpdateScanResultFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        bool,
        u32,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateInstance2: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        bool,
        u32,
        u64,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdate,
    IWindowsUpdate_Vtbl,
    0xc3c88dd7_0ef3_52b2_a9ad_66bfc6bd9582
);
impl windows_core::RuntimeType for IWindowsUpdate {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdate_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ProviderId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub UpdateId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Title: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Description: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub IsFeatureUpdate:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsMinorImpact:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsSecurity:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsCritical:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsForOS:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsDriver:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsMandatory:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsUrgent:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsSeeker:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub MoreInfoUrl: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SupportUrl: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub IsEulaAccepted:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub EulaText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Deadline: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub AttentionRequiredInfo: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ActionResult: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CurrentAction: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ActionProgress: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetPropertyValue: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub AcceptEula: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateActionCompletedEventArgs,
    IWindowsUpdateActionCompletedEventArgs_Vtbl,
    0x2c44b950_a655_5321_aec1_aee762922131
);
impl windows_core::RuntimeType for IWindowsUpdateActionCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateActionCompletedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Update: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Action: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Succeeded:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub ExtendedError: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows_core::HRESULT,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateActionProgress,
    IWindowsUpdateActionProgress_Vtbl,
    0x83b22d8a_4bb0_549f_ba39_59724882d137
);
impl windows_core::RuntimeType for IWindowsUpdateActionProgress {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateActionProgress_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Action: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Progress:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateActionResult,
    IWindowsUpdateActionResult_Vtbl,
    0xe6692c62_f697_51b7_ab7f_e73e5e688f12
);
impl windows_core::RuntimeType for IWindowsUpdateActionResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateActionResult_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Timestamp: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows::Foundation::DateTime,
    ) -> windows_core::HRESULT,
    pub Succeeded:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub ExtendedError: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows_core::HRESULT,
    ) -> windows_core::HRESULT,
    pub Action: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateAttentionRequiredInfo,
    IWindowsUpdateAttentionRequiredInfo_Vtbl,
    0x44df2579_74d3_5ffa_b6ce_09e187e1e0ed
);
impl windows_core::RuntimeType for IWindowsUpdateAttentionRequiredInfo {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateAttentionRequiredInfo_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Reason: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut WindowsUpdateAttentionRequiredReason,
    ) -> windows_core::HRESULT,
    pub Timestamp: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateAttentionRequiredReasonChangedEventArgs,
    IWindowsUpdateAttentionRequiredReasonChangedEventArgs_Vtbl,
    0x0627abca_dbb8_524a_b1d2_d9df004eeb31
);
impl windows_core::RuntimeType for IWindowsUpdateAttentionRequiredReasonChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateAttentionRequiredReasonChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Update: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Reason: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut WindowsUpdateAttentionRequiredReason,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateItem,
    IWindowsUpdateItem_Vtbl,
    0xb222e44a_49b6_59bf_a033_ef617cd73a98
);
impl windows_core::RuntimeType for IWindowsUpdateItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ProviderId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub UpdateId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Timestamp: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows::Foundation::DateTime,
    ) -> windows_core::HRESULT,
    pub Title: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Description: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub MoreInfoUrl: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Category: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Operation: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateManager,
    IWindowsUpdateManager_Vtbl,
    0x5dd966c0_a71a_5602_bbd0_09a70e4573fa
);
impl windows_core::RuntimeType for IWindowsUpdateManager {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateManager_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ScanningStateChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveScanningStateChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub WorkingStateChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveWorkingStateChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub ProgressChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveProgressChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub AttentionRequiredReasonChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveAttentionRequiredReasonChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub ActionCompleted: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveActionCompleted:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub ScanCompleted: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveScanCompleted:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub IsScanning:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsWorking:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub LastSuccessfulScanTimestamp: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetApplicableUpdates: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetMostRecentCompletedUpdates: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        i32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetMostRecentCompletedUpdatesAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        i32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub StartScan: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateManager2,
    IWindowsUpdateManager2_Vtbl,
    0xb8ba4965_423c_5e71_b89e_0b7940e7390f
);
impl windows_core::RuntimeType for IWindowsUpdateManager2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateManager2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    GetProvider: usize,
    pub ProviderIds: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut u32,
        *mut *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetApplicableSoftwareUpdates: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub PerformScan: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        WindowsUpdateManagerScanMode,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateManagerFactory,
    IWindowsUpdateManagerFactory_Vtbl,
    0x1b394df8_decb_5f44_b47c_6ccf3bcfdb37
);
impl windows_core::RuntimeType for IWindowsUpdateManagerFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateManagerFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateManagerFactory2,
    IWindowsUpdateManagerFactory2_Vtbl,
    0xba08d663_d160_59b9_9898_97a186ad52ea
);
impl windows_core::RuntimeType for IWindowsUpdateManagerFactory2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateManagerFactory2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        u32,
        *const windows_core::HSTRING,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateProgressChangedEventArgs,
    IWindowsUpdateProgressChangedEventArgs_Vtbl,
    0xbbfbdeeb_94c8_5aa7_b0fb_66c67c233b0a
);
impl windows_core::RuntimeType for IWindowsUpdateProgressChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateProgressChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Update: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ActionProgress: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowsUpdateScanCompletedEventArgs,
    IWindowsUpdateScanCompletedEventArgs_Vtbl,
    0x95b6953e_ba5c_5fe8_b115_12de184a6bb0
);
impl windows_core::RuntimeType for IWindowsUpdateScanCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
#[doc(hidden)]
pub struct IWindowsUpdateScanCompletedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ProviderId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Succeeded:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub ExtendedError: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows_core::HRESULT,
    ) -> windows_core::HRESULT,
    pub Updates: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsSoftwareUpdate(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsSoftwareUpdate,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsSoftwareUpdate {
    pub fn ProviderId(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ProviderId)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn UpdateId(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).UpdateId)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Title(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Title)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Description(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Description)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn MoreInfoUrl(&self) -> windows_core::Result<windows::Foundation::Uri> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).MoreInfoUrl)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn IsSeeker(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsSeeker)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn UpdateCategory(&self) -> windows_core::Result<WindowsSoftwareUpdateCategory> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).UpdateCategory)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn UpdateIdentity(&self) -> windows_core::Result<WindowsSoftwareUpdateIdentity> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).UpdateIdentity)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn DownloadSizeInBytes(&self) -> windows_core::Result<u64> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DownloadSizeInBytes)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn InstallSizeInBytes(&self) -> windows_core::Result<u64> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).InstallSizeInBytes)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Approve<P0>(&self, approvalinfo: P0) -> windows_core::Result<WindowsSoftwareUpdateResult>
    where
        P0: windows_core::Param<WindowsSoftwareUpdateApprovalInfo>,
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Approve)(
                windows_core::Interface::as_raw(this),
                approvalinfo.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn ApproveCurrentAction(
        &self,
        approve: bool,
    ) -> windows_core::Result<WindowsSoftwareUpdateResult> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ApproveCurrentAction)(
                windows_core::Interface::as_raw(this),
                approve,
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn CurrentAction(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CurrentAction)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn ActionResultInfo(&self) -> windows_core::Result<WindowsSoftwareUpdateActionResultInfo> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActionResultInfo)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn ApprovalInfo(&self) -> windows_core::Result<WindowsSoftwareUpdateApprovalInfo> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ApprovalInfo)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn ApprovedActions(
        &self,
    ) -> windows_core::Result<windows_collections::IVectorView<WindowsSoftwareUpdateActionType>>
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ApprovedActions)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn AttentionRequiredInfo(
        &self,
    ) -> windows_core::Result<WindowsUpdateAttentionRequiredInfo> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).AttentionRequiredInfo)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn ActionProgress(&self) -> windows_core::Result<WindowsSoftwareUpdateActionProgress> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActionProgress)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn RestartReason(
        &self,
    ) -> windows_core::Result<windows::Foundation::IReference<WindowsSoftwareUpdateRestartReason>>
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RestartReason)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn Properties(
        &self,
    ) -> windows_core::Result<
        windows_collections::IMapView<windows_core::HSTRING, windows_core::IInspectable>,
    > {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Properties)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn GetPropertyValue(
        &self,
        name: &windows_core::HSTRING,
    ) -> windows_core::Result<windows_core::IInspectable> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetPropertyValue)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(name),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    fn IWindowsSoftwareUpdateFactory<
        R,
        F: FnOnce(&IWindowsSoftwareUpdateFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            WindowsSoftwareUpdate,
            IWindowsSoftwareUpdateFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for WindowsSoftwareUpdate {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsSoftwareUpdate>();
}
unsafe impl windows_core::Interface for WindowsSoftwareUpdate {
    type Vtable = <IWindowsSoftwareUpdate as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowsSoftwareUpdate as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsSoftwareUpdate {
    const NAME: &'static str = "Windows.Management.Update.WindowsSoftwareUpdate";
}
unsafe impl Send for WindowsSoftwareUpdate {}
unsafe impl Sync for WindowsSoftwareUpdate {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateActionProgress(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsSoftwareUpdateActionProgress,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsSoftwareUpdateActionProgress {
    pub fn Action(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Action)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn CurrentProgress(&self) -> windows_core::Result<u64> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CurrentProgress)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn TotalProgress(&self) -> windows_core::Result<u64> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).TotalProgress)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateActionProgress {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsSoftwareUpdateActionProgress>();
}
unsafe impl windows_core::Interface for WindowsSoftwareUpdateActionProgress {
    type Vtable = <IWindowsSoftwareUpdateActionProgress as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsSoftwareUpdateActionProgress as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsSoftwareUpdateActionProgress {
    const NAME: &'static str = "Windows.Management.Update.WindowsSoftwareUpdateActionProgress";
}
unsafe impl Send for WindowsSoftwareUpdateActionProgress {}
unsafe impl Sync for WindowsSoftwareUpdateActionProgress {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateActionResult(pub i32);
impl WindowsSoftwareUpdateActionResult {
    pub const Succeeded: Self = Self(0i32);
    pub const Continue: Self = Self(1i32);
    pub const Failed: Self = Self(2i32);
    pub const Canceled: Self = Self(3i32);
    pub const Removed: Self = Self(4i32);
}
impl windows_core::TypeKind for WindowsSoftwareUpdateActionResult {
    type TypeKind = windows_core::CopyType;
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateActionResult {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Management.Update.WindowsSoftwareUpdateActionResult;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateActionResultInfo(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsSoftwareUpdateActionResultInfo,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsSoftwareUpdateActionResultInfo {
    pub fn Timestamp(&self) -> windows_core::Result<windows::Foundation::DateTime> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Timestamp)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Succeeded(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Succeeded)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ResultCode(&self) -> windows_core::Result<u32> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ResultCode)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ExtendedError(&self) -> windows_core::Result<u64> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ExtendedError)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Action(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Action)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateActionResultInfo {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsSoftwareUpdateActionResultInfo>();
}
unsafe impl windows_core::Interface for WindowsSoftwareUpdateActionResultInfo {
    type Vtable = <IWindowsSoftwareUpdateActionResultInfo as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsSoftwareUpdateActionResultInfo as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsSoftwareUpdateActionResultInfo {
    const NAME: &'static str = "Windows.Management.Update.WindowsSoftwareUpdateActionResultInfo";
}
unsafe impl Send for WindowsSoftwareUpdateActionResultInfo {}
unsafe impl Sync for WindowsSoftwareUpdateActionResultInfo {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateActionType(pub i32);
impl WindowsSoftwareUpdateActionType {
    pub const Download: Self = Self(0i32);
    pub const Install: Self = Self(1i32);
    pub const Deploy: Self = Self(2i32);
    pub const Reboot: Self = Self(3i32);
    pub const AppRestart: Self = Self(4i32);
}
impl windows_core::TypeKind for WindowsSoftwareUpdateActionType {
    type TypeKind = windows_core::CopyType;
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateActionType {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Management.Update.WindowsSoftwareUpdateActionType;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateApprovalInfo(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsSoftwareUpdateApprovalInfo,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsSoftwareUpdateApprovalInfo {
    pub fn UserInitiated(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).UserInitiated)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn AppClosure(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).AppClosure)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn MeteredNetwork(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).MeteredNetwork)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Seeker(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Seeker)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn CreateInstance(
        userinitiated: bool,
        appclosure: bool,
        meterednetwork: bool,
        seeker: bool,
    ) -> windows_core::Result<WindowsSoftwareUpdateApprovalInfo> {
        Self::IWindowsSoftwareUpdateApprovalInfoFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                userinitiated,
                appclosure,
                meterednetwork,
                seeker,
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    fn IWindowsSoftwareUpdateApprovalInfoFactory<
        R,
        F: FnOnce(&IWindowsSoftwareUpdateApprovalInfoFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            WindowsSoftwareUpdateApprovalInfo,
            IWindowsSoftwareUpdateApprovalInfoFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateApprovalInfo {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsSoftwareUpdateApprovalInfo>();
}
unsafe impl windows_core::Interface for WindowsSoftwareUpdateApprovalInfo {
    type Vtable = <IWindowsSoftwareUpdateApprovalInfo as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsSoftwareUpdateApprovalInfo as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsSoftwareUpdateApprovalInfo {
    const NAME: &'static str = "Windows.Management.Update.WindowsSoftwareUpdateApprovalInfo";
}
unsafe impl Send for WindowsSoftwareUpdateApprovalInfo {}
unsafe impl Sync for WindowsSoftwareUpdateApprovalInfo {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateCategory(pub i32);
impl WindowsSoftwareUpdateCategory {
    pub const Other: Self = Self(0i32);
    pub const Application: Self = Self(1i32);
    pub const Driver: Self = Self(2i32);
    pub const Firmware: Self = Self(3i32);
    pub const Definition: Self = Self(4i32);
    pub const Feature: Self = Self(5i32);
    pub const Quality: Self = Self(6i32);
    pub const AIComponent: Self = Self(7i32);
}
impl windows_core::TypeKind for WindowsSoftwareUpdateCategory {
    type TypeKind = windows_core::CopyType;
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateCategory {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Management.Update.WindowsSoftwareUpdateCategory;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateIdentity(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsSoftwareUpdateIdentity,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsSoftwareUpdateIdentity {
    pub fn Type(&self) -> windows_core::Result<WindowsSoftwareUpdateIdentityType> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Type)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Identity(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Identity)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn CreateInstance(
        r#type: WindowsSoftwareUpdateIdentityType,
        identity: &windows_core::HSTRING,
    ) -> windows_core::Result<WindowsSoftwareUpdateIdentity> {
        Self::IWindowsSoftwareUpdateIdentityFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                r#type,
                core::mem::transmute_copy(identity),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    fn IWindowsSoftwareUpdateIdentityFactory<
        R,
        F: FnOnce(&IWindowsSoftwareUpdateIdentityFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            WindowsSoftwareUpdateIdentity,
            IWindowsSoftwareUpdateIdentityFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateIdentity {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsSoftwareUpdateIdentity>();
}
unsafe impl windows_core::Interface for WindowsSoftwareUpdateIdentity {
    type Vtable = <IWindowsSoftwareUpdateIdentity as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsSoftwareUpdateIdentity as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsSoftwareUpdateIdentity {
    const NAME: &'static str = "Windows.Management.Update.WindowsSoftwareUpdateIdentity";
}
unsafe impl Send for WindowsSoftwareUpdateIdentity {}
unsafe impl Sync for WindowsSoftwareUpdateIdentity {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateIdentityType(pub i32);
impl WindowsSoftwareUpdateIdentityType {
    pub const ProductCode: Self = Self(0i32);
    pub const PackageFamilyName: Self = Self(1i32);
}
impl windows_core::TypeKind for WindowsSoftwareUpdateIdentityType {
    type TypeKind = windows_core::CopyType;
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateIdentityType {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Management.Update.WindowsSoftwareUpdateIdentityType;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateRestartReason(pub i32);
impl WindowsSoftwareUpdateRestartReason {
    pub const None: Self = Self(0i32);
    pub const System: Self = Self(1i32);
    pub const AppClose: Self = Self(2i32);
    pub const AppRestart: Self = Self(3i32);
}
impl windows_core::TypeKind for WindowsSoftwareUpdateRestartReason {
    type TypeKind = windows_core::CopyType;
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateRestartReason {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Management.Update.WindowsSoftwareUpdateRestartReason;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateResult(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsSoftwareUpdateResult,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsSoftwareUpdateResult {
    pub fn Succeeded(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Succeeded)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn CancelRequested(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CancelRequested)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ResultCode(&self) -> windows_core::Result<u32> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ResultCode)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ExtendedError(&self) -> windows_core::Result<u64> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ExtendedError)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn CreateInstance(
        succeeded: bool,
        resultcode: u32,
    ) -> windows_core::Result<WindowsSoftwareUpdateResult> {
        Self::IWindowsSoftwareUpdateResultFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                succeeded,
                resultcode,
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    pub fn CreateInstance2(
        succeeded: bool,
        resultcode: u32,
        extendederror: u64,
    ) -> windows_core::Result<WindowsSoftwareUpdateResult> {
        Self::IWindowsSoftwareUpdateResultFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance2)(
                windows_core::Interface::as_raw(this),
                succeeded,
                resultcode,
                extendederror,
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    pub fn CreateInstance3(
        succeeded: bool,
        cancelrequested: bool,
        resultcode: u32,
        extendederror: u64,
    ) -> windows_core::Result<WindowsSoftwareUpdateResult> {
        Self::IWindowsSoftwareUpdateResultFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance3)(
                windows_core::Interface::as_raw(this),
                succeeded,
                cancelrequested,
                resultcode,
                extendederror,
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    fn IWindowsSoftwareUpdateResultFactory<
        R,
        F: FnOnce(&IWindowsSoftwareUpdateResultFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            WindowsSoftwareUpdateResult,
            IWindowsSoftwareUpdateResultFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsSoftwareUpdateResult>();
}
unsafe impl windows_core::Interface for WindowsSoftwareUpdateResult {
    type Vtable = <IWindowsSoftwareUpdateResult as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowsSoftwareUpdateResult as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsSoftwareUpdateResult {
    const NAME: &'static str = "Windows.Management.Update.WindowsSoftwareUpdateResult";
}
unsafe impl Send for WindowsSoftwareUpdateResult {}
unsafe impl Sync for WindowsSoftwareUpdateResult {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsSoftwareUpdateScanResult(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsSoftwareUpdateScanResult,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsSoftwareUpdateScanResult {
    pub fn Succeeded(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Succeeded)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ResultCode(&self) -> windows_core::Result<u32> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ResultCode)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ExtendedError(&self) -> windows_core::Result<u64> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ExtendedError)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Updates(
        &self,
    ) -> windows_core::Result<windows_collections::IVectorView<WindowsSoftwareUpdate>> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Updates)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn CreateInstance<P2>(
        succeeded: bool,
        resultcode: u32,
        updates: P2,
    ) -> windows_core::Result<WindowsSoftwareUpdateScanResult>
    where
        P2: windows_core::Param<windows_collections::IIterable<WindowsSoftwareUpdate>>,
    {
        Self::IWindowsSoftwareUpdateScanResultFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                succeeded,
                resultcode,
                updates.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    pub fn CreateInstance2<P3>(
        succeeded: bool,
        resultcode: u32,
        extendederror: u64,
        updates: P3,
    ) -> windows_core::Result<WindowsSoftwareUpdateScanResult>
    where
        P3: windows_core::Param<windows_collections::IIterable<WindowsSoftwareUpdate>>,
    {
        Self::IWindowsSoftwareUpdateScanResultFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance2)(
                windows_core::Interface::as_raw(this),
                succeeded,
                resultcode,
                extendederror,
                updates.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    fn IWindowsSoftwareUpdateScanResultFactory<
        R,
        F: FnOnce(&IWindowsSoftwareUpdateScanResultFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            WindowsSoftwareUpdateScanResult,
            IWindowsSoftwareUpdateScanResultFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for WindowsSoftwareUpdateScanResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsSoftwareUpdateScanResult>();
}
unsafe impl windows_core::Interface for WindowsSoftwareUpdateScanResult {
    type Vtable = <IWindowsSoftwareUpdateScanResult as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsSoftwareUpdateScanResult as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsSoftwareUpdateScanResult {
    const NAME: &'static str = "Windows.Management.Update.WindowsSoftwareUpdateScanResult";
}
unsafe impl Send for WindowsSoftwareUpdateScanResult {}
unsafe impl Sync for WindowsSoftwareUpdateScanResult {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdate(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdate,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdate {
    pub fn ProviderId(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ProviderId)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn UpdateId(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).UpdateId)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Title(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Title)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Description(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Description)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn IsFeatureUpdate(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsFeatureUpdate)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsMinorImpact(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsMinorImpact)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsSecurity(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsSecurity)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsCritical(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsCritical)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsForOS(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsForOS)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsDriver(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsDriver)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsMandatory(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsMandatory)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsUrgent(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsUrgent)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsSeeker(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsSeeker)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn MoreInfoUrl(&self) -> windows_core::Result<windows::Foundation::Uri> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).MoreInfoUrl)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn SupportUrl(&self) -> windows_core::Result<windows::Foundation::Uri> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).SupportUrl)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn IsEulaAccepted(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsEulaAccepted)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn EulaText(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).EulaText)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Deadline(
        &self,
    ) -> windows_core::Result<windows::Foundation::IReference<windows::Foundation::DateTime>> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Deadline)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn AttentionRequiredInfo(
        &self,
    ) -> windows_core::Result<WindowsUpdateAttentionRequiredInfo> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).AttentionRequiredInfo)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn ActionResult(&self) -> windows_core::Result<WindowsUpdateActionResult> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActionResult)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn CurrentAction(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CurrentAction)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn ActionProgress(&self) -> windows_core::Result<WindowsUpdateActionProgress> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActionProgress)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn GetPropertyValue(
        &self,
        propertyname: &windows_core::HSTRING,
    ) -> windows_core::Result<windows_core::IInspectable> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetPropertyValue)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(propertyname),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn AcceptEula(&self) -> windows_core::Result<()> {
        let this = self;
        unsafe {
            (windows_core::Interface::vtable(this).AcceptEula)(windows_core::Interface::as_raw(
                this,
            ))
            .ok()
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdate {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdate>();
}
unsafe impl windows_core::Interface for WindowsUpdate {
    type Vtable = <IWindowsUpdate as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowsUpdate as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdate {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdate";
}
unsafe impl Send for WindowsUpdate {}
unsafe impl Sync for WindowsUpdate {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateActionCompletedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateActionCompletedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateActionCompletedEventArgs {
    pub fn Update(&self) -> windows_core::Result<WindowsUpdate> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Update)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn Action(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Action)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Succeeded(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Succeeded)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ExtendedError(&self) -> windows_core::Result<windows_core::HRESULT> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ExtendedError)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdateActionCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdateActionCompletedEventArgs>();
}
unsafe impl windows_core::Interface for WindowsUpdateActionCompletedEventArgs {
    type Vtable = <IWindowsUpdateActionCompletedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsUpdateActionCompletedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateActionCompletedEventArgs {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdateActionCompletedEventArgs";
}
unsafe impl Send for WindowsUpdateActionCompletedEventArgs {}
unsafe impl Sync for WindowsUpdateActionCompletedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateActionProgress(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateActionProgress,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateActionProgress {
    pub fn Action(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Action)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Progress(&self) -> windows_core::Result<f64> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Progress)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdateActionProgress {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdateActionProgress>();
}
unsafe impl windows_core::Interface for WindowsUpdateActionProgress {
    type Vtable = <IWindowsUpdateActionProgress as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowsUpdateActionProgress as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateActionProgress {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdateActionProgress";
}
unsafe impl Send for WindowsUpdateActionProgress {}
unsafe impl Sync for WindowsUpdateActionProgress {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateActionResult(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateActionResult,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateActionResult {
    pub fn Timestamp(&self) -> windows_core::Result<windows::Foundation::DateTime> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Timestamp)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Succeeded(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Succeeded)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ExtendedError(&self) -> windows_core::Result<windows_core::HRESULT> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ExtendedError)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Action(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Action)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdateActionResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdateActionResult>();
}
unsafe impl windows_core::Interface for WindowsUpdateActionResult {
    type Vtable = <IWindowsUpdateActionResult as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowsUpdateActionResult as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateActionResult {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdateActionResult";
}
unsafe impl Send for WindowsUpdateActionResult {}
unsafe impl Sync for WindowsUpdateActionResult {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateAttentionRequiredInfo(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateAttentionRequiredInfo,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateAttentionRequiredInfo {
    pub fn Reason(&self) -> windows_core::Result<WindowsUpdateAttentionRequiredReason> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Reason)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Timestamp(
        &self,
    ) -> windows_core::Result<windows::Foundation::IReference<windows::Foundation::DateTime>> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Timestamp)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdateAttentionRequiredInfo {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdateAttentionRequiredInfo>();
}
unsafe impl windows_core::Interface for WindowsUpdateAttentionRequiredInfo {
    type Vtable = <IWindowsUpdateAttentionRequiredInfo as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsUpdateAttentionRequiredInfo as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateAttentionRequiredInfo {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdateAttentionRequiredInfo";
}
unsafe impl Send for WindowsUpdateAttentionRequiredInfo {}
unsafe impl Sync for WindowsUpdateAttentionRequiredInfo {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsUpdateAttentionRequiredReason(pub i32);
impl WindowsUpdateAttentionRequiredReason {
    pub const None: Self = Self(0i32);
    pub const SeekerUpdate: Self = Self(1i32);
    pub const ReadyToReboot: Self = Self(2i32);
    pub const NeedNonMeteredNetwork: Self = Self(3i32);
    pub const NeedUserAgreementForMeteredNetwork: Self = Self(4i32);
    pub const NeedNetwork: Self = Self(5i32);
    pub const NeedMoreSpace: Self = Self(6i32);
    pub const BatterySaverEnabled: Self = Self(7i32);
    pub const NeedUserInteraction: Self = Self(8i32);
    pub const NeedUserAgreementForPolicy: Self = Self(9i32);
    pub const CompatibilityError: Self = Self(10i32);
    pub const NeedUserInteractionForEula: Self = Self(11i32);
    pub const NeedUserInteractionForCta: Self = Self(12i32);
    pub const Regulated: Self = Self(13i32);
    pub const ExternalReboot: Self = Self(14i32);
    pub const OtherUpdate: Self = Self(15i32);
    pub const BlockedByProvider: Self = Self(16i32);
    pub const BlockedByPostRebootFailure: Self = Self(17i32);
    pub const UserEngaged: Self = Self(18i32);
    pub const BlockedByBattery: Self = Self(19i32);
    pub const Exclusivity: Self = Self(20i32);
    pub const BlockedBySerialization: Self = Self(21i32);
    pub const ConflictClass: Self = Self(22i32);
    pub const BlockedByAdminApproval: Self = Self(23i32);
    pub const BlockedByTooManyAttempts: Self = Self(24i32);
    pub const BlockedByFailure: Self = Self(25i32);
    pub const Demotion: Self = Self(26i32);
    pub const BlockedByActiveHours: Self = Self(27i32);
    pub const ScheduledForMaintenance: Self = Self(28i32);
    pub const PolicyScheduledInstallTime: Self = Self(29i32);
    pub const BlockedByOobe: Self = Self(30i32);
    pub const DeferredDuringOobe: Self = Self(31i32);
    pub const DeferredForSustainableTime: Self = Self(32i32);
    pub const BlockedByAppClose: Self = Self(33i32);
    pub const BlockedByAppRestart: Self = Self(34i32);
    pub const OtherUpdateReverting: Self = Self(35i32);
    pub const RepairInProgress: Self = Self(36i32);
    pub const WaitingForLeadingUpdate: Self = Self(37i32);
    pub const Undefined: Self = Self(38i32);
}
impl windows_core::TypeKind for WindowsUpdateAttentionRequiredReason {
    type TypeKind = windows_core::CopyType;
}
impl windows_core::RuntimeType for WindowsUpdateAttentionRequiredReason {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Management.Update.WindowsUpdateAttentionRequiredReason;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateAttentionRequiredReasonChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateAttentionRequiredReasonChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateAttentionRequiredReasonChangedEventArgs {
    pub fn Update(&self) -> windows_core::Result<WindowsUpdate> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Update)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn Reason(&self) -> windows_core::Result<WindowsUpdateAttentionRequiredReason> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Reason)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdateAttentionRequiredReasonChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        IWindowsUpdateAttentionRequiredReasonChangedEventArgs,
    >();
}
unsafe impl windows_core::Interface for WindowsUpdateAttentionRequiredReasonChangedEventArgs {
    type Vtable =
        <IWindowsUpdateAttentionRequiredReasonChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsUpdateAttentionRequiredReasonChangedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateAttentionRequiredReasonChangedEventArgs {
    const NAME: &'static str =
        "Windows.Management.Update.WindowsUpdateAttentionRequiredReasonChangedEventArgs";
}
unsafe impl Send for WindowsUpdateAttentionRequiredReasonChangedEventArgs {}
unsafe impl Sync for WindowsUpdateAttentionRequiredReasonChangedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateItem {
    pub fn ProviderId(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ProviderId)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn UpdateId(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).UpdateId)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Timestamp(&self) -> windows_core::Result<windows::Foundation::DateTime> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Timestamp)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Title(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Title)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Description(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Description)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn MoreInfoUrl(&self) -> windows_core::Result<windows::Foundation::Uri> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).MoreInfoUrl)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn Category(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Category)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Operation(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Operation)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdateItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdateItem>();
}
unsafe impl windows_core::Interface for WindowsUpdateItem {
    type Vtable = <IWindowsUpdateItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowsUpdateItem as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateItem {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdateItem";
}
unsafe impl Send for WindowsUpdateItem {}
unsafe impl Sync for WindowsUpdateItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateManager(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateManager,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateManager {
    pub fn ScanningStateChanged<P0>(&self, handler: P0) -> windows_core::Result<i64>
    where
        P0: windows_core::Param<
            windows::Foundation::TypedEventHandler<
                WindowsUpdateManager,
                windows_core::IInspectable,
            >,
        >,
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ScanningStateChanged)(
                windows_core::Interface::as_raw(this),
                handler.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveScanningStateChanged(&self, token: i64) -> windows_core::Result<()> {
        let this = self;
        unsafe {
            (windows_core::Interface::vtable(this).RemoveScanningStateChanged)(
                windows_core::Interface::as_raw(this),
                token,
            )
            .ok()
        }
    }
    pub fn WorkingStateChanged<P0>(&self, handler: P0) -> windows_core::Result<i64>
    where
        P0: windows_core::Param<
            windows::Foundation::TypedEventHandler<
                WindowsUpdateManager,
                windows_core::IInspectable,
            >,
        >,
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).WorkingStateChanged)(
                windows_core::Interface::as_raw(this),
                handler.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveWorkingStateChanged(&self, token: i64) -> windows_core::Result<()> {
        let this = self;
        unsafe {
            (windows_core::Interface::vtable(this).RemoveWorkingStateChanged)(
                windows_core::Interface::as_raw(this),
                token,
            )
            .ok()
        }
    }
    pub fn ProgressChanged<P0>(&self, handler: P0) -> windows_core::Result<i64>
    where
        P0: windows_core::Param<
            windows::Foundation::TypedEventHandler<
                WindowsUpdateManager,
                WindowsUpdateProgressChangedEventArgs,
            >,
        >,
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ProgressChanged)(
                windows_core::Interface::as_raw(this),
                handler.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveProgressChanged(&self, token: i64) -> windows_core::Result<()> {
        let this = self;
        unsafe {
            (windows_core::Interface::vtable(this).RemoveProgressChanged)(
                windows_core::Interface::as_raw(this),
                token,
            )
            .ok()
        }
    }
    pub fn AttentionRequiredReasonChanged<P0>(&self, handler: P0) -> windows_core::Result<i64>
    where
        P0: windows_core::Param<
            windows::Foundation::TypedEventHandler<
                WindowsUpdateManager,
                WindowsUpdateAttentionRequiredReasonChangedEventArgs,
            >,
        >,
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).AttentionRequiredReasonChanged)(
                windows_core::Interface::as_raw(this),
                handler.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveAttentionRequiredReasonChanged(&self, token: i64) -> windows_core::Result<()> {
        let this = self;
        unsafe {
            (windows_core::Interface::vtable(this).RemoveAttentionRequiredReasonChanged)(
                windows_core::Interface::as_raw(this),
                token,
            )
            .ok()
        }
    }
    pub fn ActionCompleted<P0>(&self, handler: P0) -> windows_core::Result<i64>
    where
        P0: windows_core::Param<
            windows::Foundation::TypedEventHandler<
                WindowsUpdateManager,
                WindowsUpdateActionCompletedEventArgs,
            >,
        >,
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActionCompleted)(
                windows_core::Interface::as_raw(this),
                handler.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveActionCompleted(&self, token: i64) -> windows_core::Result<()> {
        let this = self;
        unsafe {
            (windows_core::Interface::vtable(this).RemoveActionCompleted)(
                windows_core::Interface::as_raw(this),
                token,
            )
            .ok()
        }
    }
    pub fn ScanCompleted<P0>(&self, handler: P0) -> windows_core::Result<i64>
    where
        P0: windows_core::Param<
            windows::Foundation::TypedEventHandler<
                WindowsUpdateManager,
                WindowsUpdateScanCompletedEventArgs,
            >,
        >,
    {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ScanCompleted)(
                windows_core::Interface::as_raw(this),
                handler.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveScanCompleted(&self, token: i64) -> windows_core::Result<()> {
        let this = self;
        unsafe {
            (windows_core::Interface::vtable(this).RemoveScanCompleted)(
                windows_core::Interface::as_raw(this),
                token,
            )
            .ok()
        }
    }
    pub fn IsScanning(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsScanning)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsWorking(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsWorking)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn LastSuccessfulScanTimestamp(
        &self,
    ) -> windows_core::Result<windows::Foundation::IReference<windows::Foundation::DateTime>> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).LastSuccessfulScanTimestamp)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn GetApplicableUpdates(
        &self,
    ) -> windows_core::Result<windows_collections::IVectorView<WindowsUpdate>> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetApplicableUpdates)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn GetMostRecentCompletedUpdates(
        &self,
        count: i32,
    ) -> windows_core::Result<windows_collections::IVectorView<WindowsUpdateItem>> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetMostRecentCompletedUpdates)(
                windows_core::Interface::as_raw(this),
                count,
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn GetMostRecentCompletedUpdatesAsync(
        &self,
        count: i32,
    ) -> windows_core::Result<
        windows_future::IAsyncOperation<windows_collections::IVectorView<WindowsUpdateItem>>,
    > {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetMostRecentCompletedUpdatesAsync)(
                windows_core::Interface::as_raw(this),
                count,
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn StartScan(&self, userinitiated: bool) -> windows_core::Result<()> {
        let this = self;
        unsafe {
            (windows_core::Interface::vtable(this).StartScan)(
                windows_core::Interface::as_raw(this),
                userinitiated,
            )
            .ok()
        }
    }
    pub fn ProviderIds(&self) -> windows_core::Result<windows_core::Array<windows_core::HSTRING>> {
        let this = &windows_core::Interface::cast::<IWindowsUpdateManager2>(self)?;
        unsafe {
            let mut result__ = core::mem::MaybeUninit::zeroed();
            (windows_core::Interface::vtable(this).ProviderIds)(
                windows_core::Interface::as_raw(this),
                windows_core::Array::<windows_core::HSTRING>::set_abi_len(core::mem::transmute(
                    &mut result__,
                )),
                result__.as_mut_ptr() as *mut _ as _,
            )
            .map(|| result__.assume_init())
        }
    }
    pub fn GetApplicableSoftwareUpdates(
        &self,
    ) -> windows_core::Result<windows_collections::IVectorView<WindowsSoftwareUpdate>> {
        let this = &windows_core::Interface::cast::<IWindowsUpdateManager2>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetApplicableSoftwareUpdates)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn PerformScan(
        &self,
        scanmode: WindowsUpdateManagerScanMode,
    ) -> windows_core::Result<WindowsSoftwareUpdateScanResult> {
        let this = &windows_core::Interface::cast::<IWindowsUpdateManager2>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).PerformScan)(
                windows_core::Interface::as_raw(this),
                scanmode,
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn CreateInstance(
        clientid: &windows_core::HSTRING,
    ) -> windows_core::Result<WindowsUpdateManager> {
        Self::IWindowsUpdateManagerFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(clientid),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    pub fn CreateInstance2(
        clientid: &windows_core::HSTRING,
        provideridfilter: &[windows_core::HSTRING],
    ) -> windows_core::Result<WindowsUpdateManager> {
        Self::IWindowsUpdateManagerFactory2(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(clientid),
                provideridfilter.len().try_into().unwrap(),
                core::mem::transmute(provideridfilter.as_ptr()),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        })
    }
    fn IWindowsUpdateManagerFactory<
        R,
        F: FnOnce(&IWindowsUpdateManagerFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            WindowsUpdateManager,
            IWindowsUpdateManagerFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IWindowsUpdateManagerFactory2<
        R,
        F: FnOnce(&IWindowsUpdateManagerFactory2) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            WindowsUpdateManager,
            IWindowsUpdateManagerFactory2,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for WindowsUpdateManager {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdateManager>();
}
unsafe impl windows_core::Interface for WindowsUpdateManager {
    type Vtable = <IWindowsUpdateManager as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowsUpdateManager as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateManager {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdateManager";
}
unsafe impl Send for WindowsUpdateManager {}
unsafe impl Sync for WindowsUpdateManager {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsUpdateManagerScanMode(pub i32);
impl WindowsUpdateManagerScanMode {
    pub const Default: Self = Self(0i32);
    pub const BypassScanDeferrals: Self = Self(1i32);
    pub const UserInitiated: Self = Self(2i32);
    pub const WhatIf: Self = Self(100i32);
}
impl windows_core::TypeKind for WindowsUpdateManagerScanMode {
    type TypeKind = windows_core::CopyType;
}
impl windows_core::RuntimeType for WindowsUpdateManagerScanMode {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Management.Update.WindowsUpdateManagerScanMode;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateProgressChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateProgressChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateProgressChangedEventArgs {
    pub fn Update(&self) -> windows_core::Result<WindowsUpdate> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Update)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub fn ActionProgress(&self) -> windows_core::Result<WindowsUpdateActionProgress> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActionProgress)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdateProgressChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdateProgressChangedEventArgs>();
}
unsafe impl windows_core::Interface for WindowsUpdateProgressChangedEventArgs {
    type Vtable = <IWindowsUpdateProgressChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsUpdateProgressChangedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateProgressChangedEventArgs {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdateProgressChangedEventArgs";
}
unsafe impl Send for WindowsUpdateProgressChangedEventArgs {}
unsafe impl Sync for WindowsUpdateProgressChangedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateScanCompletedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowsUpdateScanCompletedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl WindowsUpdateScanCompletedEventArgs {
    pub fn ProviderId(&self) -> windows_core::Result<windows_core::HSTRING> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ProviderId)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn Succeeded(&self) -> windows_core::Result<bool> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Succeeded)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ExtendedError(&self) -> windows_core::Result<windows_core::HRESULT> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ExtendedError)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Updates(&self) -> windows_core::Result<windows_collections::IVectorView<WindowsUpdate>> {
        let this = self;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Updates)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for WindowsUpdateScanCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowsUpdateScanCompletedEventArgs>();
}
unsafe impl windows_core::Interface for WindowsUpdateScanCompletedEventArgs {
    type Vtable = <IWindowsUpdateScanCompletedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IWindowsUpdateScanCompletedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for WindowsUpdateScanCompletedEventArgs {
    const NAME: &'static str = "Windows.Management.Update.WindowsUpdateScanCompletedEventArgs";
}
unsafe impl Send for WindowsUpdateScanCompletedEventArgs {}
unsafe impl Sync for WindowsUpdateScanCompletedEventArgs {}
